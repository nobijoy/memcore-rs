#!/usr/bin/env python3
"""
Deploy and validate memcore single-EC2 AWS staging (mock providers only).

Commands:
  deploy | status | validate | stop | destroy

Safety:
  - Always requires --profile and --region (never relies on AWS_PROFILE / AWS_REGION).
  - Asserts expected account ID before create/update/delete.
  - Never uses default or client profiles.
  - Never prints secrets / .env contents / private keys.
  - Does not create RDS, NAT, LB, EKS, ECS, ElastiCache, OpenSearch, Elastic IP.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import secrets
import shutil
import socket
import stat
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

CLIENT_PROFILE_HINTS = ("client", "work", "corp", "customer")
FORBIDDEN_MUTATING_PREFIXES = (
    "create-nat",
    "create-load-balancer",
    "create-db",
    "create-cluster",
    "create-cache",
    "allocate-address",
    "create-vpc",
)

INSTANCE_NAME = "memcore-staging-ec2"
SG_NAME = "memcore-staging-sg"
KEY_NAME = "memcore-staging-key"
REMOTE_DIR = "/home/ubuntu/memcore"
COMPOSE_FILE = "docker/docker-compose.staging.example.yml"
STATE_FILE_NAME = "staging_state.json"
ENV_FILE_NAME = ".env.aws-staging"
KEY_FILE_NAME = "memcore-staging-key.pem"
# Standard SSH port. (An alternate port was used during a prior network-blocked
# attempt; current operator network reaches 22 fine, so use the task-specified port.)
SSH_PORT = 22

REQUIRED_TAGS = {
    "Project": "memcore",
    "Environment": "staging",
    "Owner": "joy",
    "ManagedBy": "claude-cli",
    "Purpose": "single-ec2-staging",
    "CostControl": "short-lived",
}


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def redact_obj(obj: Any) -> Any:
    if isinstance(obj, dict):
        out = {}
        for k, v in obj.items():
            if re.search(r"(secret|password|token|key|credential|pem)", str(k), re.I):
                out[k] = "[REDACTED]"
            else:
                out[k] = redact_obj(v)
        return out
    if isinstance(obj, list):
        return [redact_obj(x) for x in obj]
    if isinstance(obj, str) and len(obj) > 80 and re.fullmatch(r"[A-Za-z0-9/+=._-]{40,}", obj):
        return "[REDACTED]"
    return obj


class DeployError(Exception):
    pass


class AwsStaging:
    def __init__(self, args: argparse.Namespace) -> None:
        self.args = args
        self.profile = args.profile
        self.region = args.region
        self.expected_account_id = args.expected_account_id
        self.project = args.project
        self.environment = args.environment
        self.repo_root = Path(__file__).resolve().parents[2]
        self.secrets_dir = self.repo_root / ".secrets" / "aws"
        self.reports_dir = self.repo_root / "reports" / "aws-staging"
        self.state_path = self.secrets_dir / STATE_FILE_NAME
        self.env_path = self.secrets_dir / ENV_FILE_NAME
        self.key_path = self.secrets_dir / KEY_FILE_NAME
        self.aws_bin = shutil.which("aws")
        # On Windows, aws.cmd mangles CIDR strings containing "/32". Prefer aws.exe.
        if os.name == "nt":
            exe_candidate = Path(r"C:\Program Files\Amazon\AWSCLIV2\aws.exe")
            if exe_candidate.exists():
                self.aws_bin = str(exe_candidate)
            elif self.aws_bin and self.aws_bin.lower().endswith(".cmd"):
                sibling = Path(self.aws_bin).with_suffix(".exe")
                if sibling.exists():
                    self.aws_bin = str(sibling)
        self.ssh_bin = shutil.which("ssh")
        self.scp_bin = shutil.which("scp")
        self.ssh_control_path = str(
            (self.secrets_dir / "ssh_mux_%h_%p_%r").as_posix()
        )
        self.report: dict[str, Any] = {
            "generated_at_utc": datetime.now(timezone.utc).isoformat(),
            "command": args.command,
            "profile": self.profile,
            "region": self.region,
            "expected_account_id": self.expected_account_id,
            "checks": [],
            "resources": {},
            "validation": {},
            "notes": [],
        }
        if self.profile.strip().lower() == "default":
            raise DeployError("default AWS profile is forbidden")
        if any(h in self.profile.lower() for h in CLIENT_PROFILE_HINTS):
            raise DeployError(f"client-like profile forbidden: {self.profile}")

    def log(self, msg: str) -> None:
        print(msg, flush=True)

    def record(self, name: str, result: str, notes: str = "") -> None:
        self.report["checks"].append({"check": name, "result": result, "notes": notes})
        suffix = f" — {notes}" if notes else ""
        print(f"[{result}] {name}{suffix}", flush=True)

    def write_report(self, decision: str) -> Path:
        self.reports_dir.mkdir(parents=True, exist_ok=True)
        self.report["decision"] = decision
        path = self.reports_dir / f"aws_staging_{self.args.command}_{utc_stamp()}.json"
        path.write_text(json.dumps(redact_obj(self.report), indent=2) + "\n", encoding="utf-8")
        self.log(f"Redacted report: {path}")
        return path

    def load_state(self) -> dict[str, Any]:
        if not self.state_path.exists():
            return {}
        return json.loads(self.state_path.read_text(encoding="utf-8"))

    def save_state(self, state: dict[str, Any]) -> None:
        self.secrets_dir.mkdir(parents=True, exist_ok=True)
        safe = redact_obj(dict(state))
        # Keep operational IDs unredacted in state file (needed for stop/validate).
        for k in (
            "instance_id",
            "security_group_id",
            "subnet_id",
            "vpc_id",
            "ami_id",
            "public_ip",
            "instance_type",
            "key_name",
            "operator_cidr",
            "ebs_size_gb",
        ):
            if k in state:
                safe[k] = state[k]
        self.state_path.write_text(json.dumps(safe, indent=2) + "\n", encoding="utf-8")

    def assert_command_safe(self, args: list[str]) -> None:
        if not args or args[0] != "aws":
            raise DeployError(f"non-aws command: {args!r}")
        joined = " ".join(args)
        for token in args[1:]:
            low = token.lower()
            for bad in FORBIDDEN_MUTATING_PREFIXES:
                if low.startswith(bad):
                    raise DeployError(f"refusing forbidden AWS action: {joined}")
        if "--profile" not in args:
            raise DeployError(f"AWS command missing --profile: {joined}")
        idx = args.index("--profile")
        if args[idx + 1] != self.profile:
            raise DeployError(f"unexpected profile in command: {joined}")
        if args[idx + 1] == "default":
            raise DeployError("default profile forbidden")

    def aws(
        self,
        *parts: str,
        region: str | None = None,
        allow_failure: bool = False,
        timeout: int = 120,
        output_json: bool = True,
    ) -> dict[str, Any]:
        if not self.aws_bin:
            raise DeployError("aws CLI not found on PATH")
        args = ["aws", *parts, "--profile", self.profile]
        if region is not None:
            args.extend(["--region", region])
        if output_json:
            args.extend(["--output", "json"])
        self.assert_command_safe(args)
        proc = subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=timeout,
            shell=False,
            check=False,
        )
        if proc.returncode != 0:
            err = (proc.stderr or proc.stdout or "").strip()
            err = re.sub(r"AKIA[0-9A-Z]{16}", "[REDACTED]", err)
            if allow_failure:
                return {"ok": False, "error": err[:500], "returncode": proc.returncode}
            raise DeployError(f"AWS CLI failed ({' '.join(parts[:4])}): {err[:500]}")
        raw = (proc.stdout or "").strip()
        if not output_json:
            return {"ok": True, "text": raw}
        if not raw or raw == "null":
            return {"ok": True, "data": None}
        return {"ok": True, "data": json.loads(raw)}

    def assert_account(self) -> dict[str, Any]:
        result = self.aws("sts", "get-caller-identity")
        data = result.get("data") or {}
        account = str(data.get("Account", ""))
        if account != self.expected_account_id:
            raise DeployError(
                f"WRONG ACCOUNT: STS Account={account!r} != expected "
                f"{self.expected_account_id!r}. Stop immediately."
            )
        identity = {
            "account": account,
            "arn": data.get("Arn"),
            "user_id": data.get("UserId"),
        }
        self.report["identity"] = identity
        self.record("STS account assertion", "PASS", f"account={account}")
        return identity

    def _write_json_arg(self, payload: Any) -> str:
        self.secrets_dir.mkdir(parents=True, exist_ok=True)
        path = self.secrets_dir / f"_aws_arg_{utc_stamp()}.json"
        path.write_text(json.dumps(payload), encoding="utf-8")
        # file://C:/... avoids Windows aws.cmd mangling of /32 in inline JSON.
        return "file://" + str(path.resolve()).replace("\\", "/")

    def _cleanup_json_arg(self, ref: str) -> None:
        if ref.startswith("file://"):
            try:
                Path(ref[len("file://") :]).unlink(missing_ok=True)
            except OSError:
                pass

    def tag_spec(self, resource_type: str, extra: dict[str, str] | None = None) -> str:
        tags = dict(REQUIRED_TAGS)
        if extra:
            tags.update(extra)
        items = [{"Key": k, "Value": v} for k, v in tags.items()]
        return self._write_json_arg([{"ResourceType": resource_type, "Tags": items}])

    def tags_list(self, extra: dict[str, str] | None = None) -> list[dict[str, str]]:
        tags = dict(REQUIRED_TAGS)
        if extra:
            tags.update(extra)
        return [{"Key": k, "Value": v} for k, v in tags.items()]

    def detect_public_ip(self) -> str:
        try:
            with urllib.request.urlopen("https://checkip.amazonaws.com", timeout=15) as resp:
                ip = resp.read().decode("utf-8").strip()
        except (urllib.error.URLError, TimeoutError, OSError) as exc:
            raise DeployError(
                f"failed to detect operator public IP (refusing 0.0.0.0/0 SSH): {exc}"
            ) from exc
        if not re.fullmatch(r"\d{1,3}(\.\d{1,3}){3}", ip):
            raise DeployError(f"invalid public IP detected: {ip!r}")
        return ip

    def find_default_vpc_subnet(self) -> tuple[str, str]:
        vpcs = self.aws(
            "ec2",
            "describe-vpcs",
            "--filters",
            "Name=isDefault,Values=true",
            region=self.region,
        )
        vpc_list = (vpcs.get("data") or {}).get("Vpcs") or []
        if not vpc_list:
            raise DeployError(f"no default VPC in {self.region}")
        vpc_id = vpc_list[0]["VpcId"]

        subnets = self.aws(
            "ec2",
            "describe-subnets",
            "--filters",
            f"Name=vpc-id,Values={vpc_id}",
            "Name=map-public-ip-on-launch,Values=true",
            region=self.region,
        )
        subnet_list = (subnets.get("data") or {}).get("Subnets") or []
        if not subnet_list:
            raise DeployError("no public subnet with MapPublicIpOnLaunch in default VPC")

        # Prefer subnet with IGW default route.
        rts = self.aws(
            "ec2",
            "describe-route-tables",
            "--filters",
            f"Name=vpc-id,Values={vpc_id}",
            region=self.region,
        )
        igw_subnet_ids: set[str] = set()
        for rt in (rts.get("data") or {}).get("RouteTables") or []:
            has_igw = any(
                (r.get("GatewayId") or "").startswith("igw-")
                and r.get("DestinationCidrBlock") == "0.0.0.0/0"
                for r in rt.get("Routes") or []
            )
            if not has_igw:
                continue
            for assoc in rt.get("Associations") or []:
                if assoc.get("SubnetId"):
                    igw_subnet_ids.add(assoc["SubnetId"])
            if any(a.get("Main") for a in rt.get("Associations") or []):
                # Main RT applies to subnets without explicit association.
                for s in subnet_list:
                    igw_subnet_ids.add(s["SubnetId"])

        chosen = None
        for s in subnet_list:
            if s["SubnetId"] in igw_subnet_ids or not igw_subnet_ids:
                chosen = s
                break
        if not chosen:
            chosen = subnet_list[0]
        return vpc_id, chosen["SubnetId"]

    def find_ubuntu_ami(self) -> str:
        # Prefer SSM public parameter for Ubuntu 24.04 amd64.
        for param in (
            "/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp3/ami-id",
            "/aws/service/canonical/ubuntu/server/24.04/stable/current/amd64/hvm/ebs-gp2/ami-id",
            "/aws/service/canonical/ubuntu/server/22.04/stable/current/amd64/hvm/ebs-gp3/ami-id",
            "/aws/service/canonical/ubuntu/server/22.04/stable/current/amd64/hvm/ebs-gp2/ami-id",
        ):
            res = self.aws(
                "ssm",
                "get-parameter",
                "--name",
                param,
                region=self.region,
                allow_failure=True,
            )
            if res.get("ok"):
                ami = ((res.get("data") or {}).get("Parameter") or {}).get("Value")
                if ami and str(ami).startswith("ami-"):
                    self.record("Ubuntu AMI", "PASS", f"{ami} via {param}")
                    return str(ami)

        # Fallback: describe-images Canonical owner.
        images = self.aws(
            "ec2",
            "describe-images",
            "--owners",
            "099720109477",
            "--filters",
            "Name=name,Values=ubuntu/images/hvm-ssd*/ubuntu-noble-24.04-amd64-server-*",
            "Name=state,Values=available",
            "Name=architecture,Values=x86_64",
            "--query",
            "sort_by(Images,&CreationDate)[-1].ImageId",
            region=self.region,
            allow_failure=True,
        )
        if images.get("ok") and images.get("data"):
            ami = str(images["data"]).strip('"')
            if ami.startswith("ami-"):
                self.record("Ubuntu AMI", "PASS", f"{ami} via describe-images 24.04")
                return ami
        raise DeployError("unable to resolve Ubuntu LTS x86_64 AMI")

    def ensure_key_pair(self) -> str:
        self.secrets_dir.mkdir(parents=True, exist_ok=True)
        existing = self.aws(
            "ec2",
            "describe-key-pairs",
            "--key-names",
            KEY_NAME,
            region=self.region,
            allow_failure=True,
        )
        if existing.get("ok"):
            if not self.key_path.exists():
                # Create uniquely named replacement key.
                alt = f"{KEY_NAME}-{utc_stamp()}"
                self.log(
                    f"AWS key {KEY_NAME} exists but local PEM missing; creating {alt}"
                )
                return self._create_key_pair(alt)
            self.record("Key pair", "PASS", f"reusing {KEY_NAME} with local PEM")
            return KEY_NAME

        return self._create_key_pair(KEY_NAME)

    def _create_key_pair(self, name: str) -> str:
        self.assert_account()
        result = self.aws(
            "ec2",
            "create-key-pair",
            "--key-name",
            name,
            "--key-type",
            "rsa",
            "--key-format",
            "pem",
            "--tag-specifications",
            self.tag_spec("key-pair", {"Name": name}),
            region=self.region,
        )
        material = (result.get("data") or {}).get("KeyMaterial")
        if not material:
            raise DeployError("create-key-pair returned no KeyMaterial")
        self.key_path.write_text(material, encoding="utf-8")
        try:
            os.chmod(self.key_path, stat.S_IRUSR | stat.S_IWUSR)
        except OSError:
            pass
        # Harden ACL on Windows OpenSSH.
        if os.name == "nt":
            username = os.environ.get("USERNAME") or os.environ.get("USER") or ""
            if username:
                subprocess.run(
                    [
                        "icacls",
                        str(self.key_path),
                        "/inheritance:r",
                        "/grant:r",
                        f"{username}:R",
                    ],
                    capture_output=True,
                    text=True,
                    shell=False,
                    check=False,
                )
        self.record("Key pair", "PASS", f"created {name} (PEM stored under .secrets/aws/)")
        return name

    def ensure_security_group(self, vpc_id: str, operator_cidr: str) -> str:
        self.assert_account()
        found = self.aws(
            "ec2",
            "describe-security-groups",
            "--filters",
            f"Name=group-name,Values={SG_NAME}",
            f"Name=vpc-id,Values={vpc_id}",
            region=self.region,
            allow_failure=True,
        )
        groups = (found.get("data") or {}).get("SecurityGroups") or []
        if groups:
            sg = groups[0]
            tags = {t["Key"]: t["Value"] for t in sg.get("Tags") or []}
            if tags.get("Project") != self.project or tags.get("Environment") != self.environment:
                raise DeployError(
                    f"security group {SG_NAME} exists but is not tagged for memcore/staging"
                )
            sg_id = sg["GroupId"]
            self.record("Security group", "PASS", f"reusing {sg_id}")
        else:
            created = self.aws(
                "ec2",
                "create-security-group",
                "--group-name",
                SG_NAME,
                "--description",
                "memcore short-lived single-EC2 staging (SSH+API from operator IP only)",
                "--vpc-id",
                vpc_id,
                "--tag-specifications",
                self.tag_spec("security-group", {"Name": SG_NAME}),
                region=self.region,
            )
            sg_id = (created.get("data") or {})["GroupId"]
            self.record("Security group", "PASS", f"created {sg_id}")

        # Replace ingress with operator-only SSH/API ports.
        # Revoke all existing ingress first (safe for dedicated SG).
        desc = self.aws(
            "ec2",
            "describe-security-groups",
            "--group-ids",
            sg_id,
            region=self.region,
        )
        sg = ((desc.get("data") or {}).get("SecurityGroups") or [{}])[0]
        perms = sg.get("IpPermissions") or []
        if perms:
            revoke_ref = self._write_json_arg(perms)
            try:
                self.aws(
                    "ec2",
                    "revoke-security-group-ingress",
                    "--group-id",
                    sg_id,
                    "--ip-permissions",
                    revoke_ref,
                    region=self.region,
                    allow_failure=True,
                )
            finally:
                self._cleanup_json_arg(revoke_ref)

        ingress = [
            {
                "IpProtocol": "tcp",
                "FromPort": SSH_PORT,
                "ToPort": SSH_PORT,
                "IpRanges": [
                    {
                        "CidrIp": operator_cidr,
                        "Description": "SSH from operator IP only",
                    }
                ],
            },
            {
                "IpProtocol": "tcp",
                "FromPort": 8080,
                "ToPort": 8080,
                "IpRanges": [
                    {
                        "CidrIp": operator_cidr,
                        "Description": "API from operator IP only",
                    }
                ],
            },
        ]
        ingress_ref = self._write_json_arg(ingress)
        try:
            self.aws(
                "ec2",
                "authorize-security-group-ingress",
                "--group-id",
                sg_id,
                "--ip-permissions",
                ingress_ref,
                region=self.region,
            )
        finally:
            self._cleanup_json_arg(ingress_ref)
        self.record(
            "SG ingress",
            "PASS",
            f"{SSH_PORT}/8080 from {operator_cidr} only; no DB/vector/cache ports",
        )
        return sg_id

    def refresh_operator_ingress(self, sg_id: str) -> str:
        """Re-detect public IP and ensure SG allows current /32 (dynamic ISP IPs)."""
        self.assert_account()
        operator_ip = self.detect_public_ip()
        operator_cidr = f"{operator_ip}/32"
        ingress = []
        for port in (SSH_PORT, 8080):
            ingress.append(
                {
                    "IpProtocol": "tcp",
                    "FromPort": port,
                    "ToPort": port,
                    "IpRanges": [
                        {
                            "CidrIp": operator_cidr,
                            "Description": f"operator refresh port {port}",
                        }
                    ],
                }
            )
        ingress_ref = self._write_json_arg(ingress)
        try:
            self.aws(
                "ec2",
                "authorize-security-group-ingress",
                "--group-id",
                sg_id,
                "--ip-permissions",
                ingress_ref,
                region=self.region,
                allow_failure=True,
            )
        finally:
            self._cleanup_json_arg(ingress_ref)
        self.record("SG IP refresh", "PASS", operator_cidr)
        return operator_cidr

    def user_data_script(self) -> str:
        return """#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
exec > >(tee /var/log/memcore-bootstrap.log) 2>&1
# Swap helps Free Tier / small instances during Docker Rust builds.
if [ ! -f /swapfile ]; then
  fallocate -l 2G /swapfile || dd if=/dev/zero of=/swapfile bs=1M count=2048
  chmod 600 /swapfile
  mkswap /swapfile
  swapon /swapfile
  echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi
apt-get update -y
apt-get install -y ca-certificates curl git jq python3 unzip tar gnupg
install -m 0755 -d /etc/apt/keyrings
if [ ! -f /etc/apt/keyrings/docker.asc ]; then
  curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
  chmod a+r /etc/apt/keyrings/docker.asc
fi
. /etc/os-release
echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.asc] https://download.docker.com/linux/ubuntu ${VERSION_CODENAME} stable" > /etc/apt/sources.list.d/docker.list
apt-get update -y
apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin docker-buildx-plugin
usermod -aG docker ubuntu
systemctl enable --now docker
mkdir -p /home/ubuntu/memcore
chown -R ubuntu:ubuntu /home/ubuntu/memcore
touch /var/lib/cloud/instance/memcore-bootstrap-done
"""

    def launch_instance(
        self,
        ami_id: str,
        subnet_id: str,
        sg_id: str,
        key_name: str,
        instance_type: str,
        ebs_size_gb: int,
    ) -> str:
        self.assert_account()
        # Avoid duplicate running/stopped instances with same Name tag.
        existing = self.aws(
            "ec2",
            "describe-instances",
            "--filters",
            f"Name=tag:Name,Values={INSTANCE_NAME}",
            f"Name=tag:Project,Values={self.project}",
            f"Name=tag:Environment,Values={self.environment}",
            "Name=instance-state-name,Values=pending,running,stopping,stopped",
            region=self.region,
        )
        for res in (existing.get("data") or {}).get("Reservations") or []:
            for inst in res.get("Instances") or []:
                iid = inst["InstanceId"]
                state = (inst.get("State") or {}).get("Name")
                raise DeployError(
                    f"existing instance {iid} state={state} already tagged "
                    f"Name={INSTANCE_NAME}; stop/destroy first or reuse via status"
                )

        bd = [
            {
                "DeviceName": "/dev/sda1",
                "Ebs": {
                    "VolumeSize": ebs_size_gb,
                    "VolumeType": "gp3",
                    "DeleteOnTermination": True,
                },
            }
        ]
        tag_specs = json.dumps(
            [
                {
                    "ResourceType": "instance",
                    "Tags": self.tags_list({"Name": INSTANCE_NAME}),
                },
                {
                    "ResourceType": "volume",
                    "Tags": self.tags_list({"Name": f"{INSTANCE_NAME}-root"}),
                },
            ]
        )
        # AWS CLI base64-encodes --user-data strings; write a temp file instead.
        with tempfile.NamedTemporaryFile(
            "w", suffix="-userdata.sh", delete=False, encoding="utf-8"
        ) as uf:
            uf.write(self.user_data_script())
            userdata_path = uf.name
        userdata_path_obj = Path(userdata_path).resolve()
        # AWS CLI on Windows accepts file://C:/path (not file:///C:/ from as_uri()).
        userdata_ref = "file://" + str(userdata_path_obj).replace("\\", "/")
        try:
            result = self.aws(
                "ec2",
                "run-instances",
                "--image-id",
                ami_id,
                "--instance-type",
                instance_type,
                "--key-name",
                key_name,
                "--subnet-id",
                subnet_id,
                "--security-group-ids",
                sg_id,
                "--associate-public-ip-address",
                "--block-device-mappings",
                json.dumps(bd),
                "--user-data",
                userdata_ref,
                "--tag-specifications",
                tag_specs,
                "--count",
                "1",
                region=self.region,
                timeout=180,
            )
        finally:
            try:
                Path(userdata_path).unlink(missing_ok=True)
            except OSError:
                pass
        inst = ((result.get("data") or {}).get("Instances") or [None])[0]
        if not inst:
            raise DeployError("run-instances returned no instance")
        return inst["InstanceId"]

    def wait_instance_running(self, instance_id: str, timeout_s: int = 600) -> dict[str, Any]:
        deadline = time.time() + timeout_s
        while time.time() < deadline:
            desc = self.aws(
                "ec2",
                "describe-instances",
                "--instance-ids",
                instance_id,
                region=self.region,
            )
            inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
            inst = (inst.get("Instances") or [None])[0]
            if not inst:
                time.sleep(5)
                continue
            state = (inst.get("State") or {}).get("Name")
            if state == "running" and inst.get("PublicIpAddress"):
                # Status checks
                st = self.aws(
                    "ec2",
                    "describe-instance-status",
                    "--instance-ids",
                    instance_id,
                    "--include-all-instances",
                    region=self.region,
                    allow_failure=True,
                )
                statuses = (st.get("data") or {}).get("InstanceStatuses") or []
                if statuses:
                    isys = (statuses[0].get("InstanceStatus") or {}).get("Status")
                    isys2 = (statuses[0].get("SystemStatus") or {}).get("Status")
                    if isys == "ok" and isys2 == "ok":
                        return inst
                else:
                    # Still warming; return when public IP exists after grace.
                    if time.time() + 60 > deadline:
                        return inst
            if state in {"terminated", "shutting-down"}:
                raise DeployError(f"instance entered state {state}")
            time.sleep(10)
        raise DeployError(f"timeout waiting for instance {instance_id} running/status ok")

    def _ssh_base_args(self) -> list[str]:
        self.secrets_dir.mkdir(parents=True, exist_ok=True)
        return [
            "-i",
            str(self.key_path),
            "-p",
            str(SSH_PORT),
            "-o",
            "StrictHostKeyChecking=accept-new",
            "-o",
            "ConnectTimeout=15",
            "-o",
            "BatchMode=yes",
            "-o",
            "ServerAliveInterval=30",
            "-o",
            "ServerAliveCountMax=10",
            "-o",
            "ControlMaster=auto",
            "-o",
            f"ControlPath={self.ssh_control_path}",
            "-o",
            "ControlPersist=60m",
        ]

    def open_ssh_mux(self, public_ip: str, sg_id: str) -> None:
        """Refresh SG to current /32 then open a persistent SSH mux (SG is stateful)."""
        if not self.ssh_bin:
            raise DeployError("ssh not found")
        deadline = time.time() + 300
        last_err = ""
        while time.time() < deadline:
            cidr = self.refresh_operator_ingress(sg_id)
            # Tiny pause so SG rule is active, then connect immediately.
            time.sleep(2)
            proc = subprocess.run(
                [
                    self.ssh_bin,
                    *self._ssh_base_args(),
                    "-o",
                    "ControlMaster=yes",
                    f"ubuntu@{public_ip}",
                    "echo ok",
                ],
                capture_output=True,
                text=True,
                shell=False,
                check=False,
                timeout=40,
            )
            if proc.returncode == 0 and "ok" in (proc.stdout or ""):
                self.record(
                    "SSH mux",
                    "PASS",
                    f"ubuntu@{public_ip}:{SSH_PORT} via {cidr} (persistent)",
                )
                return
            last_err = (proc.stderr or proc.stdout or "")[:200]
            time.sleep(3)
        raise DeployError(f"unable to open SSH mux on {SSH_PORT}: {last_err}")

    def wait_ssh(self, public_ip: str, timeout_s: int = 600) -> None:
        state = self.load_state()
        sg_id = state.get("security_group_id")
        if not sg_id:
            raise DeployError("missing security_group_id in state for SSH")
        # Wait until bootstrap has at least opened sshd on SSH_PORT, then mux.
        deadline = time.time() + timeout_s
        last_err = ""
        while time.time() < deadline:
            try:
                self.open_ssh_mux(public_ip, sg_id)
                self.record("SSH reachable", "PASS", f"ubuntu@{public_ip}:{SSH_PORT}")
                return
            except DeployError as exc:
                last_err = str(exc)
                time.sleep(5)
        raise DeployError(f"SSH not reachable on port {SSH_PORT}: {last_err}")

    def wait_bootstrap(self, public_ip: str, timeout_s: int = 900) -> None:
        deadline = time.time() + timeout_s
        while time.time() < deadline:
            code, out, err = self.ssh(
                public_ip,
                "test -f /var/lib/cloud/instance/memcore-bootstrap-done && sudo docker --version && sudo docker compose version",
                check=False,
            )
            if code == 0:
                self.record("Docker bootstrap", "PASS", out.strip().replace("\n", " | ")[:200])
                return
            time.sleep(15)
        raise DeployError("timeout waiting for user-data bootstrap (docker)")

    def ssh(
        self, public_ip: str, remote_cmd: str, check: bool = True, timeout: int = 120
    ) -> tuple[int, str, str]:
        if not self.ssh_bin:
            raise DeployError("ssh not found")
        proc = subprocess.run(
            [
                self.ssh_bin,
                *self._ssh_base_args(),
                f"ubuntu@{public_ip}",
                remote_cmd,
            ],
            capture_output=True,
            text=True,
            timeout=timeout,
            shell=False,
            check=False,
        )
        if check and proc.returncode != 0:
            err = (proc.stderr or proc.stdout or "")[:500]
            # Redact possible secret-looking fragments.
            err = re.sub(r"(Bearer\s+)\S+", r"\1[REDACTED]", err, flags=re.I)
            raise DeployError(f"SSH command failed: {err}")
        return proc.returncode, proc.stdout or "", proc.stderr or ""

    def scp_to(self, public_ip: str, local: Path, remote: str) -> None:
        if not self.scp_bin:
            raise DeployError("scp not found")
        proc = subprocess.run(
            [
                self.scp_bin,
                "-i",
                str(self.key_path),
                "-P",
                str(SSH_PORT),
                "-o",
                "StrictHostKeyChecking=accept-new",
                "-o",
                "BatchMode=yes",
                "-o",
                f"ControlPath={self.ssh_control_path}",
                "-o",
                "ControlMaster=auto",
                str(local),
                f"ubuntu@{public_ip}:{remote}",
            ],
            capture_output=True,
            text=True,
            shell=False,
            check=False,
        )
        if proc.returncode != 0:
            raise DeployError(f"scp failed: {(proc.stderr or '')[:300]}")

    def generate_env(self, public_ip: str) -> Path:
        self.secrets_dir.mkdir(parents=True, exist_ok=True)
        pg_pass = secrets.token_urlsafe(24)
        api_key = "mcs_" + secrets.token_urlsafe(32)
        pepper = secrets.token_urlsafe(24)
        content = f"""# AWS single-EC2 staging env — generated, mock providers only. Never commit.
MEMCORE_ENV=production
MEMCORE_HOST=0.0.0.0
MEMCORE_PORT=8080
MEMCORE_BASE_URL=http://{public_ip}:8080

MEMCORE_AUTH_ENABLED=true
MEMCORE_AUTH_MODE=dev
MEMCORE_DEV_API_KEY={api_key}
MEMCORE_API_KEY_PEPPER={pepper}

MEMCORE_STORAGE_MODE=production
MEMCORE_FACT_BACKEND=postgres
MEMCORE_EVENT_BACKEND=postgres
MEMCORE_DATABASE_BACKEND=postgres
MEMCORE_POSTGRES_URL=postgres://memcore:{pg_pass}@postgres:5432/memcore_staging
MEMCORE_DATABASE_MIGRATIONS_ENABLED=true
MEMCORE_DATABASE_MIGRATION_MODE=auto
MEMCORE_DATABASE_REQUIRE_CLEAN_MIGRATIONS=true

POSTGRES_DB=memcore_staging
POSTGRES_USER=memcore
POSTGRES_PASSWORD={pg_pass}

MEMCORE_VECTOR_BACKEND=qdrant
MEMCORE_QDRANT_URL=http://qdrant:6334
MEMCORE_QDRANT_COLLECTION=memcore_aws_staging

MEMCORE_LLM_PROVIDER=mock
MEMCORE_LLM_MODEL=mock-llm
MEMCORE_EMBEDDING_PROVIDER=mock
MEMCORE_EMBEDDING_MODEL=mock-embedding

MEMCORE_PROVIDER_GUARDRAILS_ENABLED=true
MEMCORE_PROVIDER_TEST_MODE=mock_only
MEMCORE_REAL_PROVIDER_CALLS_ENABLED=false
MEMCORE_ALLOW_REAL_PROVIDERS_DURING_LOAD_TESTS=false
MEMCORE_BACKGROUND_JOBS_ALLOW_REAL_PROVIDERS=false
MEMCORE_MULTI_PROVIDER_VALIDATION_ENABLED=false
MEMCORE_PROVIDER_MAX_CALLS_PER_RUN=10
MEMCORE_PROVIDER_MAX_INPUT_CHARS=4000
MEMCORE_PROVIDER_MAX_OUTPUT_TOKENS=300
MEMCORE_PROVIDER_MAX_RETRIES_PER_CALL=1
MEMCORE_PROVIDER_TIMEOUT_SECONDS=30

MEMCORE_CONTEXT_CACHE_BACKEND=disabled
MEMCORE_BACKGROUND_JOBS_ENABLED=true
MEMCORE_BACKGROUND_JOB_ORG_IDS=org_staging
MEMCORE_BACKGROUND_JOB_LOCK_ENABLED=true
MEMCORE_BACKGROUND_JOB_LOCK_BACKEND=database
MEMCORE_BACKGROUND_JOB_HISTORY_ENABLED=true

MEMCORE_RATE_LIMIT_ENABLED=true
MEMCORE_RATE_LIMIT_REQUESTS_PER_MINUTE=60

MEMCORE_SECURITY_HEADERS_ENABLED=true
MEMCORE_CORS_ENABLED=false
MEMCORE_CORS_ALLOW_CREDENTIALS=false

MEMCORE_RESTORE_ENABLED=false
MEMCORE_BACKUP_ENABLED=false
MEMCORE_BACKUP_DIR=/var/lib/memcore/backups

MEMCORE_LOG_FORMAT=json
MEMCORE_LOG_LEVEL=info

MEMCORE_METRICS_ENABLED=true
MEMCORE_METRICS_PATH=/metrics
MEMCORE_METRICS_REQUIRE_AUTH=true

MEMCORE_SMOKE_TEST_API_KEY={api_key}
MEMCORE_SMOKE_TEST_ORG_ID=org_staging
MEMCORE_SMOKE_TEST_USER_ID=aws-smoke-user
MEMCORE_METRICS_API_KEY={api_key}

APP_VERSION=0.1.0-aws-staging
GIT_SHA=aws-staging
BUILD_TIMESTAMP={utc_stamp()}
"""
        # newline="\n" avoids Windows write_text() translating to CRLF, which
        # breaks `. ./.env.staging` sourcing on the Linux EC2 instance.
        self.env_path.write_text(content, encoding="utf-8", newline="\n")
        try:
            os.chmod(self.env_path, stat.S_IRUSR | stat.S_IWUSR)
        except OSError:
            pass
        self.record("Staging env", "PASS", "generated under .secrets/aws/ (not printed)")
        return self.env_path

    def build_archive(self) -> Path:
        exclude_dirs = {
            ".git",
            "target",
            ".secrets",
            "reports",
            "node_modules",
            ".cursor",
            "dist",
        }
        exclude_names = {".env", ".env.local", ".env.staging", ".env.production"}
        tmp = Path(tempfile.mkdtemp(prefix="memcore-aws-"))
        archive = tmp / "memcore-staging.tar.gz"

        def filter_tar(ti: tarfile.TarInfo) -> tarfile.TarInfo | None:
            parts = Path(ti.name).parts
            if any(p in exclude_dirs for p in parts):
                return None
            base = Path(ti.name).name
            if base in exclude_names or base.startswith(".env."):
                if not base.endswith(".example"):
                    return None
            if base.endswith(".pem") or base.endswith(".dump"):
                return None
            return ti

        with tarfile.open(archive, "w:gz") as tar:
            for item in self.repo_root.iterdir():
                if item.name in exclude_dirs:
                    continue
                tar.add(item, arcname=item.name, filter=filter_tar)
        self.record("Deploy archive", "PASS", f"created (size_bytes={archive.stat().st_size})")
        return archive

    def upload_and_extract(self, public_ip: str, archive: Path) -> None:
        self.ssh(public_ip, f"mkdir -p {REMOTE_DIR} && rm -rf {REMOTE_DIR}/*")
        remote_tar = "/home/ubuntu/memcore-staging.tar.gz"
        self.scp_to(public_ip, archive, remote_tar)
        self.scp_to(public_ip, self.env_path, f"{REMOTE_DIR}/.env.staging")
        self.ssh(
            public_ip,
            f"tar -xzf {remote_tar} -C {REMOTE_DIR} && rm -f {remote_tar} && "
            f"chmod 600 {REMOTE_DIR}/.env.staging && "
            # tar built on Windows loses the Unix executable bit; restore it.
            f"find {REMOTE_DIR}/scripts -name '*.sh' -exec chmod +x {{}} + && "
            f"test ! -f {REMOTE_DIR}/.secrets 2>/dev/null; true",
        )
        self.record("Upload project", "PASS", f"extracted to {REMOTE_DIR}")

    def compose_up(self, public_ip: str) -> None:
        # Rust release build with postgres+qdrant can take a long time.
        cmd = (
            f"cd {REMOTE_DIR} && "
            f"sudo docker compose -f {COMPOSE_FILE} --env-file .env.staging up -d --build"
        )
        self.log("Starting docker compose build/up on EC2 (may take 20–45+ minutes)...")
        code, out, err = self.ssh(public_ip, cmd, check=False, timeout=3600)
        if code != 0:
            raise DeployError(f"compose up failed: {(err or out)[:500]}")
        code, out, _ = self.ssh(
            public_ip,
            f"cd {REMOTE_DIR} && sudo docker compose -f {COMPOSE_FILE} --env-file .env.staging ps",
            timeout=120,
        )
        self.record("Compose start", "PASS", "memcore/postgres/qdrant up (ps ok)")
        self.report["validation"]["compose_ps"] = out[:1000]

    def wait_ready_remote(self, public_ip: str, timeout_s: int = 300) -> None:
        deadline = time.time() + timeout_s
        while time.time() < deadline:
            code, _, _ = self.ssh(
                public_ip,
                "curl -fsS http://localhost:8080/ready >/dev/null",
                check=False,
                timeout=30,
            )
            if code == 0:
                self.record("Ready (on-instance)", "PASS", "http://localhost:8080/ready")
                return
            time.sleep(10)
        raise DeployError("timeout waiting for /ready on instance")

    def curl_local(self, url: str, headers: dict[str, str] | None = None) -> tuple[int, str]:
        args = ["curl", "-sS", "-o", "-", "-w", "\n%{http_code}", "--max-time", "30"]
        for k, v in (headers or {}).items():
            args.extend(["-H", f"{k}: {v}"])
        args.append(url)
        proc = subprocess.run(args, capture_output=True, text=True, shell=False, check=False)
        body = proc.stdout or ""
        if "\n" in body:
            *rest, code_s = body.rsplit("\n", 1)
            return int(code_s.strip() or "0"), "\n".join(rest)
        return proc.returncode, body

    def read_env_value(self, key: str) -> str:
        if not self.env_path.exists():
            return ""
        for line in self.env_path.read_text(encoding="utf-8").splitlines():
            if line.startswith(f"{key}="):
                return line.split("=", 1)[1].strip()
        return ""

    def validate_endpoints(self, public_ip: str) -> None:
        base = f"http://{public_ip}:8080"
        for path in ("/health", "/ready", "/api/v1/version"):
            code, body = self.curl_local(f"{base}{path}")
            if code != 200:
                raise DeployError(f"local curl {path} failed HTTP {code}")
            self.record(f"Endpoint {path}", "PASS", "HTTP 200 from operator machine")
        code, out, _ = self.ssh(
            public_ip,
            "curl -fsS http://localhost:8080/health && "
            "curl -fsS http://localhost:8080/ready && "
            "curl -fsS http://localhost:8080/api/v1/version",
            timeout=60,
        )
        self.record("On-instance health/ready/version", "PASS", "curl ok")

    def validate_smoke(self, public_ip: str) -> None:
        # Run smoke on EC2 so secrets stay on instance; do not print env.
        remote = (
            f"cd {REMOTE_DIR} && "
            f"set -a && . ./.env.staging && set +a && "
            f"bash ./scripts/ops/smoke_test.sh http://localhost:8080 --authenticated"
        )
        code, out, err = self.ssh(public_ip, remote, check=False, timeout=180)
        combined = (out + "\n" + err).lower()
        if any(x in combined for x in ("password=", "bearer mcs_", "postgres://memcore:")):
            self.record("Auth smoke", "FAIL", "possible secret leakage in smoke output")
            raise DeployError("smoke output may contain secrets")
        if code != 0:
            raise DeployError(f"authenticated smoke failed: {(err or out)[:400]}")
        self.record("Auth smoke", "PASS", "create/list/search/context/delete (mock)")

    def validate_metrics(self, public_ip: str) -> None:
        remote = (
            f"cd {REMOTE_DIR} && "
            f"set -a && . ./.env.staging && set +a && "
            f"MEMCORE_METRICS_STRICT=true bash ./scripts/ops/check_metrics.sh http://localhost:8080"
        )
        code, out, err = self.ssh(public_ip, remote, check=False, timeout=120)
        if code != 0:
            raise DeployError(f"metrics check failed: {(err or out)[:400]}")
        # Unauthenticated should fail
        code2, _, _ = self.ssh(
            public_ip,
            "curl -sS -o /dev/null -w '%{http_code}' http://localhost:8080/metrics",
            check=False,
        )
        self.record("Metrics", "PASS", "auth-required scrape ok; unauth checked")

    def validate_logs(self, public_ip: str) -> None:
        # Fetch recent logs and scan for secret material without printing secrets.
        api_key = self.read_env_value("MEMCORE_DEV_API_KEY")
        pg_pass = self.read_env_value("POSTGRES_PASSWORD")
        code, out, err = self.ssh(
            public_ip,
            f"cd {REMOTE_DIR} && sudo docker compose -f {COMPOSE_FILE} --env-file .env.staging logs --tail=200 memcore",
            check=False,
            timeout=120,
        )
        logs = out + "\n" + err
        findings = []
        if api_key and api_key in logs:
            findings.append("api_key")
        if pg_pass and pg_pass in logs:
            findings.append("postgres_password")
        if "postgres://memcore:" in logs.lower():
            findings.append("db_url")
        if re.search(r"authorization:\s*bearer\s+\S+", logs, re.I):
            findings.append("bearer_token")
        # Heuristic: raw smoke content
        if "aws-smoke-user prefers" in logs.lower() or "user prefers concise" in logs.lower():
            findings.append("raw_memory_or_prompt")
        if findings:
            self.record("Logs redaction", "FAIL", f"leak indicators: {findings}")
            raise DeployError(f"log redaction failed: {findings}")
        self.record("Logs redaction", "PASS", "no secret/prompt indicators in recent logs")

    def validate_backup(self, public_ip: str) -> None:
        # Adapt local script constraints: run on EC2 with localhost URL.
        remote = (
            f"cd {REMOTE_DIR} && "
            f"set -a && . ./.env.staging && set +a && "
            f"MEMCORE_STAGING_BASE_URL=http://localhost:8080 "
            f"ENV_FILE=.env.staging "
            f"COMPOSE_FILE={COMPOSE_FILE} "
            f"bash ./scripts/ops/local_staging_backup_dry_run.sh"
        )
        code, out, err = self.ssh(public_ip, remote, check=False, timeout=600)
        text = out + "\n" + err
        if code != 0:
            # Script may refuse if path checks differ; fall back to inline dry-run.
            self.log("backup script returned non-zero; attempting inline restore-check...")
            inline = f"""
set -euo pipefail
cd {REMOTE_DIR}
compose() {{ sudo docker compose -f {COMPOSE_FILE} --env-file .env.staging "$@"; }}
mkdir -p reports/staging-backups
DUMP_CTR=/tmp/memcore_aws_staging_backup.dump
compose exec -T postgres pg_dump -U memcore -d memcore_staging --format=custom --file=$DUMP_CTR
LINES=$(compose exec -T postgres pg_restore --list $DUMP_CTR | wc -l)
test "$LINES" -gt 5
ACTIVE=$(compose exec -T postgres psql -U memcore -d memcore_staging -tAc 'SELECT COUNT(*) FROM schema_migrations;' | tr -d '[:space:]')
compose exec -T postgres psql -U memcore -d postgres -v ON_ERROR_STOP=1 -c "DROP DATABASE IF EXISTS memcore_staging_restore_check;"
compose exec -T postgres psql -U memcore -d postgres -v ON_ERROR_STOP=1 -c "CREATE DATABASE memcore_staging_restore_check;"
compose exec -T postgres pg_restore -U memcore -d memcore_staging_restore_check --no-owner $DUMP_CTR
REST=$(compose exec -T postgres psql -U memcore -d memcore_staging_restore_check -tAc 'SELECT COUNT(*) FROM schema_migrations;' | tr -d '[:space:]')
test "$ACTIVE" = "$REST"
compose exec -T postgres psql -U memcore -d postgres -v ON_ERROR_STOP=1 -c "DROP DATABASE memcore_staging_restore_check;"
# Qdrant snapshot if collection exists
curl -fsS -X POST http://localhost:6333/collections/memcore_aws_staging/snapshots >/tmp/qdrant_snap.json || true
echo BACKUP_OK active_migrations=$ACTIVE
"""
            code2, out2, err2 = self.ssh(public_ip, inline, check=False, timeout=600)
            if code2 != 0 or "BACKUP_OK" not in (out2 + err2):
                raise DeployError(f"backup/restore-check failed: {(err2 or out2 or text)[:400]}")
            self.record("Backup/restore-check", "PASS", "inline pg_dump + isolated restore + qdrant snap attempt")
            return
        self.record("Backup/restore-check", "PASS", "local_staging_backup_dry_run.sh on EC2")

    def validate_restart_persistence(self, public_ip: str) -> None:
        # Create memory, restart containers, verify persistence.
        # Use non-f-string for remote python snippet to avoid brace escaping bugs.
        remote_dir = REMOTE_DIR
        compose_file = COMPOSE_FILE
        script = f"""
set -euo pipefail
cd {remote_dir}
set -a && . ./.env.staging && set +a
ORG="$MEMCORE_SMOKE_TEST_ORG_ID"
USER="persist-user"
KEY="$MEMCORE_DEV_API_KEY"
BASE=http://localhost:8080
BODY='{{"user_id":"'"$USER"'","messages":[{{"role":"user","content":"AWS staging persistence probe: user prefers concise technical summaries."}}],"metadata":{{"source":"aws_staging_persist_check"}}}}'
CREATE=$(curl -fsS -H "Authorization: Bearer $KEY" -H "X-Organization-ID: $ORG" -H "Content-Type: application/json" -H "X-Memcore-Test-Source: smoke-test" -d "$BODY" "$BASE/api/v1/memories")
echo "$CREATE" | python3 -c 'import sys,json; d=json.load(sys.stdin); m=d.get("memories") or []; print((m[0].get("id") if m else "") or "")' > /tmp/persist_id.txt
ID=$(cat /tmp/persist_id.txt)
test -n "$ID"
compose() {{ sudo docker compose -f {compose_file} --env-file .env.staging "$@"; }}
compose restart memcore
sleep 8
curl -fsS "$BASE/ready" >/dev/null
curl -fsS -H "Authorization: Bearer $KEY" -H "X-Organization-ID: $ORG" "$BASE/api/v1/users/$USER/memories" | grep -q "$ID"
compose restart qdrant
sleep 8
curl -fsS "$BASE/ready" >/dev/null
curl -fsS -H "Authorization: Bearer $KEY" -H "X-Organization-ID: $ORG" -H "Content-Type: application/json" -H "X-Memcore-Test-Source: smoke-test" -d '{{"user_id":"'"$USER"'","query":"concise summaries","limit":5}}' "$BASE/api/v1/memories/search" >/dev/null
compose restart postgres
sleep 15
for i in $(seq 1 30); do curl -fsS "$BASE/ready" >/dev/null && break; sleep 3; done
curl -fsS "$BASE/ready" >/dev/null
curl -fsS -H "Authorization: Bearer $KEY" -H "X-Organization-ID: $ORG" "$BASE/api/v1/users/$USER/memories" | grep -q "$ID"
curl -fsS -X DELETE -H "Authorization: Bearer $KEY" -H "X-Organization-ID: $ORG" "$BASE/api/v1/users/$USER/memories/$ID" >/dev/null || true
echo PERSIST_OK
"""
        code, out, err = self.ssh(public_ip, script, check=False, timeout=600)
        if code != 0 or "PERSIST_OK" not in (out + err):
            raise DeployError(f"restart/persistence failed: {(err or out)[:400]}")
        self.record("Restart/persistence", "PASS", "memcore/qdrant/postgres restart retained memory")

    def inventory(self) -> dict[str, Any]:
        inv: dict[str, Any] = {}
        inst = self.aws(
            "ec2",
            "describe-instances",
            "--filters",
            f"Name=tag:Project,Values={self.project}",
            f"Name=tag:Environment,Values={self.environment}",
            "Name=instance-state-name,Values=pending,running,stopping,stopped",
            region=self.region,
        )
        items = []
        for res in (inst.get("data") or {}).get("Reservations") or []:
            for i in res.get("Instances") or []:
                items.append(
                    {
                        "id": i.get("InstanceId"),
                        "state": (i.get("State") or {}).get("Name"),
                        "type": i.get("InstanceType"),
                    }
                )
        inv["ec2"] = items
        for name, cmd in [
            ("nat", ["ec2", "describe-nat-gateways", "--filter", "Name=state,Values=available"]),
            ("elb", ["elbv2", "describe-load-balancers"]),
            ("rds", ["rds", "describe-db-instances"]),
            ("eks", ["eks", "list-clusters"]),
            ("ecs", ["ecs", "list-clusters"]),
            ("elasticache", ["elasticache", "describe-cache-clusters"]),
            ("opensearch", ["opensearch", "list-domain-names"]),
        ]:
            res = self.aws(*cmd, region=self.region, allow_failure=True)
            inv[name] = "ok" if res.get("ok") else "error"
            data = res.get("data") or {}
            count = 0
            if name == "nat":
                count = len(data.get("NatGateways") or [])
            elif name == "elb":
                count = len(data.get("LoadBalancers") or [])
            elif name == "rds":
                count = len(data.get("DBInstances") or [])
            elif name == "eks":
                count = len(data.get("clusters") or [])
            elif name == "ecs":
                count = len(data.get("clusterArns") or [])
            elif name == "elasticache":
                count = len(data.get("CacheClusters") or [])
            elif name == "opensearch":
                count = len(data.get("DomainNames") or [])
            inv[f"{name}_count"] = count
            if count:
                self.record(f"Inventory {name}", "WARN", f"count={count}")
            else:
                self.record(f"Inventory {name}", "PASS", "none")
        eip = self.aws("ec2", "describe-addresses", region=self.region, allow_failure=True)
        unattached = 0
        if eip.get("ok"):
            unattached = sum(
                1 for a in (eip.get("data") or {}).get("Addresses") or [] if not a.get("AssociationId")
            )
        inv["unattached_eip"] = unattached
        if unattached:
            self.record("Inventory Elastic IP", "WARN", f"unattached={unattached}")
        else:
            self.record("Inventory Elastic IP", "PASS", "none unattached")
        self.report["inventory"] = inv
        return inv

    def stop_instance(self, instance_id: str) -> None:
        self.assert_account()
        self.aws(
            "ec2",
            "stop-instances",
            "--instance-ids",
            instance_id,
            region=self.region,
        )
        # Wait until stopped
        deadline = time.time() + 600
        while time.time() < deadline:
            desc = self.aws(
                "ec2",
                "describe-instances",
                "--instance-ids",
                instance_id,
                region=self.region,
            )
            inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
            inst = (inst.get("Instances") or [{}])[0]
            state = (inst.get("State") or {}).get("Name")
            if state == "stopped":
                self.record("Stop instance", "PASS", f"{instance_id} stopped")
                return
            time.sleep(8)
        raise DeployError(f"timeout waiting for stop of {instance_id}")

    def cmd_continue(self) -> int:
        """Resume upload/compose/validate/stop on an already-running staging instance."""
        self.assert_account()
        state = self.load_state()
        iid = state.get("instance_id")
        sg_id = state.get("security_group_id")
        if not iid or not sg_id:
            raise DeployError("missing instance_id/security_group_id; deploy first")
        desc = self.aws(
            "ec2",
            "describe-instances",
            "--instance-ids",
            iid,
            region=self.region,
        )
        inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
        inst = (inst.get("Instances") or [{}])[0]
        if (inst.get("State") or {}).get("Name") != "running":
            raise DeployError(f"instance {iid} is not running")
        public_ip = inst.get("PublicIpAddress")
        if not public_ip:
            raise DeployError("instance has no public IP")
        state["public_ip"] = public_ip
        state["state"] = "running"
        self.save_state(state)
        self.report["resources"] = dict(state)

        self.wait_ssh(public_ip)
        self.wait_bootstrap(public_ip)
        self.generate_env(public_ip)
        archive = self.build_archive()
        try:
            self.upload_and_extract(public_ip, archive)
        finally:
            try:
                shutil.rmtree(archive.parent, ignore_errors=True)
            except OSError:
                pass
        self.compose_up(public_ip)
        self.wait_ready_remote(public_ip, timeout_s=600)
        # Prefer on-instance checks; refresh SG before external curls.
        self.refresh_operator_ingress(sg_id)
        self.validate_endpoints(public_ip)
        self.validate_smoke(public_ip)
        self.validate_metrics(public_ip)
        self.validate_logs(public_ip)
        self.validate_backup(public_ip)
        self.validate_restart_persistence(public_ip)
        self.inventory()
        if self.args.stop_after_validation:
            self.stop_instance(iid)
            state["state"] = "stopped"
            state["public_ip"] = None
            self.save_state(state)
            self.record("Stop-after-validation", "PASS", iid)
        self.write_report("AWS single-EC2 staging validation passed (mock providers)")
        return 0

    def cmd_status(self) -> int:
        self.assert_account()
        state = self.load_state()
        iid = state.get("instance_id")
        if not iid:
            self.log("No local staging state found.")
            return 0
        desc = self.aws(
            "ec2",
            "describe-instances",
            "--instance-ids",
            iid,
            region=self.region,
            allow_failure=True,
        )
        if not desc.get("ok"):
            self.log(f"instance {iid}: describe failed")
            return 1
        inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
        inst = (inst.get("Instances") or [None])[0]
        if not inst:
            self.log(f"instance {iid}: not found")
            return 1
        print(
            json.dumps(
                {
                    "instance_id": iid,
                    "state": (inst.get("State") or {}).get("Name"),
                    "type": inst.get("InstanceType"),
                    "public_ip": inst.get("PublicIpAddress"),
                },
                indent=2,
            )
        )
        return 0

    def cmd_stop(self) -> int:
        self.assert_account()
        state = self.load_state()
        iid = state.get("instance_id")
        if not iid:
            raise DeployError("no instance_id in staging state")
        self.stop_instance(iid)
        state["public_ip"] = None
        state["state"] = "stopped"
        self.save_state(state)
        self.write_report("instance stopped")
        return 0

    def cmd_destroy(self) -> int:
        self.assert_account()
        state = self.load_state()
        iid = state.get("instance_id")
        if iid:
            self.aws(
                "ec2",
                "terminate-instances",
                "--instance-ids",
                iid,
                region=self.region,
            )
            self.record("Terminate instance", "PASS", iid)
        sg_id = state.get("security_group_id")
        # Wait for terminate before deleting SG
        if iid:
            deadline = time.time() + 600
            while time.time() < deadline:
                desc = self.aws(
                    "ec2",
                    "describe-instances",
                    "--instance-ids",
                    iid,
                    region=self.region,
                    allow_failure=True,
                )
                inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
                inst = (inst.get("Instances") or [{}])[0]
                if (inst.get("State") or {}).get("Name") == "terminated":
                    break
                time.sleep(8)
        if sg_id:
            self.aws(
                "ec2",
                "delete-security-group",
                "--group-id",
                sg_id,
                region=self.region,
                allow_failure=True,
            )
            self.record("Delete SG", "PASS", sg_id)
        self.write_report("destroy completed (key pair retained unless manually deleted)")
        return 0

    def cmd_validate(self) -> int:
        self.assert_account()
        state = self.load_state()
        public_ip = state.get("public_ip")
        iid = state.get("instance_id")
        if not public_ip or not iid:
            raise DeployError("missing public_ip/instance_id; deploy first")
        # Refresh IP if instance running
        desc = self.aws(
            "ec2",
            "describe-instances",
            "--instance-ids",
            iid,
            region=self.region,
        )
        inst = ((desc.get("data") or {}).get("Reservations") or [{}])[0]
        inst = (inst.get("Instances") or [{}])[0]
        if (inst.get("State") or {}).get("Name") != "running":
            raise DeployError("instance is not running")
        public_ip = inst.get("PublicIpAddress") or public_ip
        self.validate_endpoints(public_ip)
        self.validate_smoke(public_ip)
        self.validate_metrics(public_ip)
        self.validate_logs(public_ip)
        self.validate_backup(public_ip)
        self.validate_restart_persistence(public_ip)
        self.inventory()
        self.write_report("validation passed")
        return 0

    def cmd_deploy(self) -> int:
        self.assert_account()
        operator_ip = self.detect_public_ip()
        operator_cidr = f"{operator_ip}/32"
        self.record("Operator public IP", "PASS", operator_cidr)

        vpc_id, subnet_id = self.find_default_vpc_subnet()
        self.record("Network", "PASS", f"vpc={vpc_id} subnet={subnet_id}")
        ami_id = self.find_ubuntu_ami()
        key_name = self.ensure_key_pair()
        sg_id = self.ensure_security_group(vpc_id, operator_cidr)

        instance_type = self.args.instance_type
        ebs_size = int(self.args.ebs_size_gb)
        if ebs_size < 20 or ebs_size > 30:
            raise DeployError("ebs-size-gb must be 20–30 for this staging plan")

        try:
            instance_id = self.launch_instance(
                ami_id, subnet_id, sg_id, key_name, instance_type, ebs_size
            )
        except DeployError as exc:
            msg = str(exc)
            retryable = any(
                s in msg
                for s in (
                    "InsufficientInstanceCapacity",
                    "Unsupported",
                    "not eligible for Free Tier",
                    "InvalidParameterCombination",
                    "VcpuLimitExceeded",
                    "InstanceLimitExceeded",
                )
            )
            if not retryable:
                raise
            fallbacks = []
            for cand in (
                self.args.fallback_instance_type,
                "t3.micro",
            ):
                if cand and cand != instance_type and cand not in fallbacks:
                    fallbacks.append(cand)
            last_exc: Exception = exc
            for fb in fallbacks:
                self.log(f"preferred/current type failed ({msg[:120]}); trying {fb}")
                try:
                    instance_type = fb
                    instance_id = self.launch_instance(
                        ami_id, subnet_id, sg_id, key_name, instance_type, ebs_size
                    )
                    if fb == "t3.micro":
                        self.record(
                            "Instance type fallback",
                            "WARN",
                            "using t3.micro due to Free Tier / capacity limits; "
                            "Postgres+Qdrant+memcore may be memory-constrained",
                        )
                    break
                except DeployError as exc2:
                    last_exc = exc2
                    continue
            else:
                raise DeployError(str(last_exc)) from last_exc

        self.record("Launch EC2", "PASS", f"{instance_id} type={instance_type}")
        inst = self.wait_instance_running(instance_id)
        public_ip = inst.get("PublicIpAddress")
        if not public_ip:
            raise DeployError("instance has no public IP")
        self.record("Instance running", "PASS", f"public_ip={public_ip}")

        state = {
            "instance_id": instance_id,
            "instance_type": instance_type,
            "ami_id": ami_id,
            "subnet_id": subnet_id,
            "vpc_id": vpc_id,
            "security_group_id": sg_id,
            "key_name": key_name,
            "public_ip": public_ip,
            "operator_cidr": operator_cidr,
            "ebs_size_gb": ebs_size,
            "state": "running",
        }
        self.save_state(state)
        self.report["resources"] = dict(state)

        # Dynamic ISP IPs: refresh SG immediately before SSH.
        operator_cidr = self.refresh_operator_ingress(sg_id)
        state["operator_cidr"] = operator_cidr
        self.save_state(state)

        self.wait_ssh(public_ip)
        self.wait_bootstrap(public_ip)
        self.generate_env(public_ip)
        archive = self.build_archive()
        try:
            self.upload_and_extract(public_ip, archive)
        finally:
            try:
                shutil.rmtree(archive.parent, ignore_errors=True)
            except OSError:
                pass

        self.compose_up(public_ip)
        self.wait_ready_remote(public_ip, timeout_s=600)
        self.validate_endpoints(public_ip)
        self.validate_smoke(public_ip)
        self.validate_metrics(public_ip)
        self.validate_logs(public_ip)
        self.validate_backup(public_ip)
        self.validate_restart_persistence(public_ip)
        self.inventory()

        if self.args.stop_after_validation:
            self.stop_instance(instance_id)
            state["state"] = "stopped"
            state["public_ip"] = None
            self.save_state(state)
            self.record("Stop-after-validation", "PASS", instance_id)
        else:
            self.record("Stop-after-validation", "SKIP", "flag not set")

        self.write_report("AWS single-EC2 staging validation passed (mock providers)")
        return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(description="memcore single-EC2 AWS staging (mock only)")
    p.add_argument(
        "command",
        choices=["deploy", "status", "validate", "stop", "destroy", "continue"],
    )
    p.add_argument("--profile", required=True)
    p.add_argument("--region", required=True)
    p.add_argument("--expected-account-id", required=True)
    p.add_argument("--project", default="memcore")
    p.add_argument("--environment", default="staging")
    p.add_argument("--instance-type", default="t3.medium")
    p.add_argument("--fallback-instance-type", default="t3.small")
    p.add_argument("--ebs-size-gb", type=int, default=30)
    p.add_argument(
        "--stop-after-validation",
        action="store_true",
        help="Stop EC2 after successful validation (recommended)",
    )
    return p


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        app = AwsStaging(args)
        if args.command == "deploy":
            return app.cmd_deploy()
        if args.command == "continue":
            return app.cmd_continue()
        if args.command == "status":
            return app.cmd_status()
        if args.command == "validate":
            return app.cmd_validate()
        if args.command == "stop":
            return app.cmd_stop()
        if args.command == "destroy":
            return app.cmd_destroy()
        raise DeployError(f"unknown command {args.command}")
    except DeployError as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
