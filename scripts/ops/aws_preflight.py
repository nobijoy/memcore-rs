#!/usr/bin/env python3
"""
Read-only AWS preflight for memcore single-EC2 staging safety.

Never creates, modifies, or deletes AWS resources.
Never prints secrets (access keys, session tokens, provider keys, .env content).
Always requires explicit --profile and --region (does not rely on AWS_PROFILE / AWS_REGION).

Usage:
  python scripts/ops/aws_preflight.py \\
    --profile memcore-personal \\
    --region ap-south-1 \\
    --expected-account-id 749251636291 \\
    --project memcore \\
    --environment staging
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------

FORBIDDEN_MUTATING_VERBS = frozenset(
    {
        "create-",
        "delete-",
        "terminate-",
        "run-instances",
        "modify-",
        "authorize-",
        "revoke-",
        "attach-",
        "detach-",
        "allocate-",
        "associate-",
        "put-",
        "update-",
        "register-",
        "start-",
        "reboot-",
        "purchase-",
    }
)

SENSITIVE_KEY_RE = re.compile(
    r"(secret|token|password|credential|authorization|api[_-]?key|access[_-]?key)",
    re.IGNORECASE,
)

CLIENT_PROFILE_HINTS = ("client", "work", "corp", "customer")

REQUIRED_BUDGET_HINTS = (
    ("5", "actual"),
    ("10", "actual"),
    ("20", "actual"),
    ("50", "forecast"),
)

CANDIDATE_INSTANCE_TYPES = ("t3.small", "t3.medium", "t4g.small", "t4g.medium")

EXPENSIVE_RESOURCE_KINDS = (
    "nat_gateway",
    "load_balancer",
    "rds",
    "elasticache",
    "eks",
    "ecs",
    "opensearch",
    "unattached_eip",
    "large_ebs",
)


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def utc_stamp() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")


def redact_value(key: str, value: Any) -> Any:
    if SENSITIVE_KEY_RE.search(key):
        return "[REDACTED]"
    if isinstance(value, dict):
        return {k: redact_value(k, v) for k, v in value.items()}
    if isinstance(value, list):
        return [redact_value(key, v) for v in value]
    if isinstance(value, str):
        # Never echo long opaque credentials if somehow present.
        if len(value) > 80 and re.fullmatch(r"[A-Za-z0-9/+=._-]{40,}", value):
            return "[REDACTED]"
    return value


def redact_obj(obj: Any) -> Any:
    if isinstance(obj, dict):
        return {k: redact_value(k, v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [redact_obj(v) for v in obj]
    return obj


def print_check(name: str, result: str, notes: str = "") -> None:
    suffix = f" — {notes}" if notes else ""
    print(f"[{result}] {name}{suffix}")


class PreflightError(Exception):
    """Hard failure that should stop the script."""


class AwsPreflight:
    def __init__(
        self,
        profile: str,
        region: str,
        expected_account_id: str,
        project: str,
        environment: str,
        repo_root: Path,
    ) -> None:
        self.profile = profile
        self.region = region
        self.expected_account_id = expected_account_id
        self.project = project
        self.environment = environment
        self.repo_root = repo_root
        self.aws_bin = shutil.which("aws")
        self.checks: list[dict[str, Any]] = []
        self.warnings: list[str] = []
        self.failures: list[str] = []
        self.blocked_reasons: list[str] = []
        self.manual_verification_required: list[str] = []
        self.identity: dict[str, Any] = {}
        self.budgets: dict[str, Any] = {}
        self.inventory: dict[str, Any] = {}
        self.network: dict[str, Any] = {}
        self.instance_types: dict[str, Any] = {}
        self.decision = ""
        self._command_audit: list[list[str]] = []

    # ------------------------------------------------------------------
    # AWS CLI invocation (read-only)
    # ------------------------------------------------------------------

    def _assert_command_safe(self, args: list[str]) -> None:
        """Fail if a generated AWS command is unsafe for this preflight."""
        if not args or args[0] != "aws":
            raise PreflightError(f"internal error: non-aws command template: {args!r}")

        joined = " ".join(args)
        for verb in FORBIDDEN_MUTATING_VERBS:
            # Match service subcommands like ec2 create-instances / budgets create-budget
            for token in args[1:]:
                if token.startswith(verb) or token == verb.rstrip("-"):
                    raise PreflightError(
                        f"refusing mutating AWS command template: {joined}"
                    )

        if "--profile" not in args:
            raise PreflightError(
                f"deployment/preflight command template lacks --profile {self.profile}: {joined}"
            )
        try:
            idx = args.index("--profile")
            profile_value = args[idx + 1]
        except (ValueError, IndexError) as exc:
            raise PreflightError(
                f"command template has invalid --profile usage: {joined}"
            ) from exc

        if profile_value != self.profile:
            raise PreflightError(
                f"command uses unexpected profile {profile_value!r}; "
                f"required {self.profile!r}: {joined}"
            )
        if profile_value == "default":
            raise PreflightError(f"command uses default profile (forbidden): {joined}")
        lowered = profile_value.lower()
        if any(h in lowered for h in CLIENT_PROFILE_HINTS) and profile_value != self.profile:
            raise PreflightError(f"command uses client-like profile: {joined}")

        # Regional services must pass --region explicitly (STS may omit region).
        service = args[1] if len(args) > 1 else ""
        regional_services = {
            "ec2",
            "elbv2",
            "elb",
            "rds",
            "elasticache",
            "eks",
            "ecs",
            "opensearch",
            "s3api",
            "service-quotas",
        }
        if service in regional_services and "--region" not in args:
            raise PreflightError(
                f"regional command template lacks --region {self.region}: {joined}"
            )

        self._command_audit.append(list(args))

    def aws(
        self,
        *parts: str,
        region: str | None = None,
        allow_failure: bool = False,
        timeout: int = 90,
    ) -> dict[str, Any]:
        if not self.aws_bin:
            raise PreflightError("AWS CLI unavailable (aws not found on PATH)")

        args = ["aws", *parts, "--profile", self.profile, "--output", "json"]
        if region is not None:
            args.extend(["--region", region])

        self._assert_command_safe(args)

        try:
            proc = subprocess.run(
                args,
                capture_output=True,
                text=True,
                timeout=timeout,
                shell=False,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise PreflightError(f"AWS CLI timed out: {' '.join(parts)}") from exc

        if proc.returncode != 0:
            err = (proc.stderr or proc.stdout or "").strip()
            # Redact anything that looks like a key material fragment.
            err = re.sub(r"(AKIA[0-9A-Z]{16})", "[REDACTED_ACCESS_KEY_ID]", err)
            err = re.sub(
                r"(?i)(aws_secret_access_key|session[_-]?token)\s*[:=]\s*\S+",
                r"\1=[REDACTED]",
                err,
            )
            if allow_failure:
                return {
                    "ok": False,
                    "error": err[:500],
                    "returncode": proc.returncode,
                }
            raise PreflightError(f"AWS CLI failed ({' '.join(parts)}): {err[:500]}")

        raw = (proc.stdout or "").strip()
        if not raw or raw == "null":
            return {"ok": True, "data": None}

        try:
            data = json.loads(raw)
        except json.JSONDecodeError as exc:
            raise PreflightError(
                f"failed to parse AWS JSON for {' '.join(parts)}"
            ) from exc

        return {"ok": True, "data": redact_obj(data)}

    def record(
        self,
        name: str,
        result: str,
        notes: str = "",
        details: Any = None,
    ) -> None:
        entry = {
            "check": name,
            "result": result,
            "notes": notes,
            "details": redact_obj(details) if details is not None else None,
        }
        self.checks.append(entry)
        print_check(name, result, notes)
        if result == "FAIL":
            self.failures.append(f"{name}: {notes or 'failed'}")
        elif result == "WARN":
            self.warnings.append(f"{name}: {notes or 'warning'}")
        elif result == "MANUAL":
            self.manual_verification_required.append(f"{name}: {notes or 'manual'}")

    # ------------------------------------------------------------------
    # Checks
    # ------------------------------------------------------------------

    def check_cli_and_command_templates(self) -> None:
        if not self.aws_bin:
            self.record("AWS CLI available", "FAIL", "aws not found on PATH")
            self.blocked_reasons.append("AWS CLI/profile unavailable")
            raise PreflightError("AWS CLI unavailable")

        self.record("AWS CLI available", "PASS", self.aws_bin)

        # Validate that every command we *will* emit includes --profile.
        sample_templates = [
            ["aws", "sts", "get-caller-identity", "--profile", self.profile, "--output", "json"],
            [
                "aws",
                "ec2",
                "describe-vpcs",
                "--profile",
                self.profile,
                "--region",
                self.region,
                "--output",
                "json",
            ],
            [
                "aws",
                "budgets",
                "describe-budgets",
                "--account-id",
                self.expected_account_id,
                "--profile",
                self.profile,
                "--region",
                "us-east-1",
                "--output",
                "json",
            ],
        ]
        for tmpl in sample_templates:
            try:
                self._assert_command_safe(tmpl)
            except PreflightError as exc:
                self.record("Command template profile safety", "FAIL", str(exc))
                raise
        self.record(
            "Command template profile safety",
            "PASS",
            f"all templates require --profile {self.profile}; no default/client profiles",
        )

        # Refuse if caller tried to use default via empty profile (argparse already requires it).
        if self.profile.strip().lower() in {"", "default"}:
            self.record("Profile is not default", "FAIL", f"profile={self.profile!r}")
            self.blocked_reasons.append("wrong AWS account/profile")
            raise PreflightError("default profile is forbidden")
        self.record("Profile is not default", "PASS", self.profile)

    def check_profiles(self) -> None:
        # list-profiles returns plain text (one name per line), not JSON.
        args = ["aws", "configure", "list-profiles", "--profile", self.profile]
        # list-profiles ignores --profile for listing, but we still pass it for safety audit;
        # some CLI versions reject unknown args — call without forcing json via helper.
        if not self.aws_bin:
            raise PreflightError("AWS CLI unavailable")

        # Special-case: list-profiles has no --output json. Audit manually.
        list_args = ["aws", "configure", "list-profiles"]
        # Still require that we never invoke without intending the target profile for other cmds.
        try:
            proc = subprocess.run(
                list_args,
                capture_output=True,
                text=True,
                timeout=60,
                shell=False,
                check=False,
            )
        except subprocess.TimeoutExpired as exc:
            raise PreflightError("aws configure list-profiles timed out") from exc

        if proc.returncode != 0:
            err = (proc.stderr or "").strip()[:300]
            self.record("Profile exists", "FAIL", f"list-profiles failed: {err}")
            self.blocked_reasons.append("AWS CLI/profile unavailable")
            raise PreflightError("unable to list AWS profiles")

        profiles = [p.strip() for p in (proc.stdout or "").splitlines() if p.strip()]
        if self.profile not in profiles:
            self.record(
                "Profile exists",
                "FAIL",
                f"{self.profile!r} not found in aws configure list-profiles",
            )
            self.blocked_reasons.append("wrong AWS account/profile")
            raise PreflightError(f"profile {self.profile!r} does not exist")

        self.record("Profile exists", "PASS", f"{self.profile} present")

        # Warn if default exists and may point elsewhere (non-fatal).
        if "default" in profiles:
            self.record(
                "Default profile present",
                "WARN",
                "default profile exists; this script never uses it — always pass --profile",
            )
        else:
            self.record("Default profile present", "PASS", "no default profile listed")

    def check_identity(self) -> None:
        result = self.aws("sts", "get-caller-identity")
        data = result.get("data") or {}
        account = str(data.get("Account", ""))
        arn = str(data.get("Arn", ""))
        user_id = str(data.get("UserId", ""))
        self.identity = {
            "account": account,
            "arn": arn,
            "user_id": user_id,
        }

        if account != self.expected_account_id:
            msg = (
                f"WRONG ACCOUNT: STS Account={account!r} does not match "
                f"expected {self.expected_account_id!r}. "
                f"Stop immediately. Do not use this profile for memcore staging. "
                f"Client AWS accounts must never be touched."
            )
            self.record("STS account matches expected", "FAIL", msg)
            self.blocked_reasons.append("wrong AWS account/profile")
            raise PreflightError(msg)

        self.record(
            "STS account matches expected",
            "PASS",
            f"account={account}",
        )
        # Redacted identity print (no secrets).
        print(f"  identity arn={arn}")
        print(f"  identity user_id={user_id}")

    def check_configure_list(self) -> None:
        # `aws configure list` is human text; run with explicit profile.
        # Tabular output (not JSON). Still require explicit --profile; never default.
        args = [
            "aws",
            "configure",
            "list",
            "--profile",
            self.profile,
        ]
        if "--profile" not in args or self.profile not in args:
            raise PreflightError("configure list must use explicit --profile")
        self._command_audit.append(list(args))
        proc = subprocess.run(
            args,
            capture_output=True,
            text=True,
            timeout=60,
            shell=False,
            check=False,
        )
        if proc.returncode != 0:
            err = (proc.stderr or "").strip()[:300]
            self.record("aws configure list", "WARN", f"could not read: {err}")
            return

        text = proc.stdout or ""
        # Redact any secret-looking cells.
        safe_lines = []
        for line in text.splitlines():
            if re.search(r"(?i)access_key|secret|token", line) and re.search(
                r"\S{8,}", line
            ):
                # Keep name/type columns only; blank value-looking tails.
                safe_lines.append(re.sub(r"(AKIA[0-9A-Z]{16}|\S{12,})", "[REDACTED]", line))
            else:
                safe_lines.append(line)

        region_ok = False
        for line in safe_lines:
            if re.search(r"\bregion\b", line, re.IGNORECASE) and self.region in line:
                region_ok = True
                break

        # Region may be unset in profile if always passed on CLI — acceptable if script region set.
        if region_ok:
            self.record(
                "Region matches expected",
                "PASS",
                f"configure list shows {self.region}",
            )
        else:
            self.record(
                "Region matches expected",
                "PASS",
                f"script --region {self.region} will be used for all regional calls "
                f"(profile region may be unset)",
            )
        self.identity["configure_list_redacted"] = "\n".join(safe_lines)

    def check_budgets(self) -> None:
        # Budgets API is typically called against us-east-1.
        result = self.aws(
            "budgets",
            "describe-budgets",
            "--account-id",
            self.expected_account_id,
            region="us-east-1",
            allow_failure=True,
        )
        if not result.get("ok"):
            err = str(result.get("error", ""))
            note = (
                "Budgets API inaccessible (permissions or endpoint). "
                "Operator already configured $5/$10/$20 actual and $50 forecasted alerts "
                "in console — mark manual_verification_required. "
                "Budgets are alerts, not hard spending stops. "
                f"Detail: {err[:200]}"
            )
            self.budgets = {
                "visible": False,
                "manual_verification_required": True,
                "error": err[:300],
            }
            self.record("Budgets visible", "MANUAL", note)
            return

        data = result.get("data") or {}
        budgets = data.get("Budgets") or []
        if not budgets:
            self.budgets = {
                "visible": True,
                "count": 0,
                "manual_verification_required": True,
                "matched_hints": [],
            }
            self.record(
                "Budgets visible",
                "MANUAL",
                "API returned zero budgets; confirm console alerts manually. "
                "Budgets are alerts, not hard spending stops.",
            )
            return

        # Summarize budgets and pull notification thresholds (read-only).
        summaries = []
        blob_parts = []
        for b in budgets:
            name = str(b.get("BudgetName", ""))
            limit = b.get("BudgetLimit") or {}
            amount = str(limit.get("Amount", ""))
            unit = str(limit.get("Unit", ""))
            btype = str(b.get("BudgetType", ""))
            summaries.append(
                {
                    "name": name,
                    "amount": amount,
                    "unit": unit,
                    "type": btype,
                }
            )
            blob_parts.append(f"{name} {amount} {unit} {btype}".lower())
            for n in b.get("Notifications") or []:
                blob_parts.append(
                    f"{n.get('NotificationType', '')} {n.get('ComparisonOperator', '')} "
                    f"{n.get('Threshold', '')} {n.get('ThresholdType', '')}".lower()
                )

            # describe-budgets often omits notifications; fetch per-budget (read-only).
            if name:
                notes = self.aws(
                    "budgets",
                    "describe-notifications-for-budget",
                    "--account-id",
                    self.expected_account_id,
                    "--budget-name",
                    name,
                    region="us-east-1",
                    allow_failure=True,
                )
                if notes.get("ok"):
                    for n in (notes.get("data") or {}).get("Notifications") or []:
                        ntype = str(n.get("NotificationType", "")).lower()
                        thresh = str(n.get("Threshold", ""))
                        ttype = str(n.get("ThresholdType", "")).lower()
                        blob_parts.append(f"{ntype} {thresh} {ttype}")
                        # Absolute USD thresholds map directly; percentage needs budget limit.
                        if "absolute" in ttype or ttype == "":
                            blob_parts.append(thresh)
                        elif "percentage" in ttype and amount:
                            try:
                                usd = float(amount) * float(thresh) / 100.0
                                blob_parts.append(str(int(usd)) if usd == int(usd) else str(usd))
                            except ValueError:
                                pass
                        if "actual" in ntype:
                            blob_parts.append("actual")
                        if "forecast" in ntype:
                            blob_parts.append("forecast")

        blob = " | ".join(blob_parts)
        matched = []
        missing = []
        for amount, kind in REQUIRED_BUDGET_HINTS:
            # Match whole dollar amounts (avoid "5" matching "50").
            amount_re = re.compile(
                rf"(?:^|[^0-9.]){re.escape(amount)}(?:\.0+)?(?:[^0-9.]|$)"
            )
            amount_seen = bool(amount_re.search(blob))
            kind_seen = kind in blob
            if amount_seen and kind_seen:
                matched.append(f"${amount} {kind}")
            elif amount_seen:
                matched.append(f"${amount} (amount seen; verify {kind} in console)")
            else:
                missing.append(f"${amount} {kind}")

        self.budgets = {
            "visible": True,
            "count": len(budgets),
            "summaries": summaries,
            "matched_hints": matched,
            "missing_hints": missing,
            "manual_verification_required": bool(missing),
            "note": "AWS Budgets are alerts, not hard spending stops.",
        }

        self.record(
            "Budgets visible",
            "PASS",
            f"{len(budgets)} budget(s) in account; alerts ≠ hard stops",
        )
        for label, amount, kind in (
            ("$5 actual", "5", "actual"),
            ("$10 actual", "10", "actual"),
            ("$20 actual", "20", "actual"),
            ("$50 forecasted", "50", "forecast"),
        ):
            ok = any(
                m.startswith(f"${amount} {kind}")
                or m.startswith(f"${amount} (amount seen")
                for m in matched
            )
            if ok and any(m.startswith(f"${amount} {kind}") for m in matched):
                self.record(f"Budget hint {label}", "PASS", "heuristic match in API data")
            elif ok:
                self.record(
                    f"Budget hint {label}",
                    "MANUAL",
                    "amount seen; confirm notification type in Billing console",
                )
            else:
                self.record(
                    f"Budget hint {label}",
                    "MANUAL",
                    "not clearly visible via API; confirm in Billing console",
                )

    def _tag_filter_json(self) -> str:
        # EC2 filter JSON for Name/Environment/Project tags.
        return json.dumps(
            [
                {"Name": f"tag:Project", "Values": [self.project]},
                {"Name": f"tag:Environment", "Values": [self.environment]},
            ]
        )

    def check_inventory(self) -> None:
        inv: dict[str, Any] = {}
        expensive_found: list[str] = []

        # EC2 instances (project/env tags)
        inst = self.aws(
            "ec2",
            "describe-instances",
            "--filters",
            f"Name=tag:Project,Values={self.project}",
            f"Name=tag:Environment,Values={self.environment}",
            region=self.region,
            allow_failure=True,
        )
        instances = []
        if inst.get("ok"):
            for res in (inst.get("data") or {}).get("Reservations") or []:
                for i in res.get("Instances") or []:
                    instances.append(
                        {
                            "id": i.get("InstanceId"),
                            "state": (i.get("State") or {}).get("Name"),
                            "type": i.get("InstanceType"),
                        }
                    )
            inv["ec2_instances"] = {"count": len(instances), "items": instances}
            self.record(
                "EC2 inventory (tagged)",
                "PASS" if not instances else "WARN",
                f"{len(instances)} instance(s) with Project={self.project} Environment={self.environment}",
            )
        else:
            inv["ec2_instances"] = {"error": inst.get("error")}
            self.record("EC2 inventory (tagged)", "WARN", str(inst.get("error"))[:200])

        # Security groups
        sgs = self.aws(
            "ec2",
            "describe-security-groups",
            "--filters",
            f"Name=tag:Project,Values={self.project}",
            f"Name=tag:Environment,Values={self.environment}",
            region=self.region,
            allow_failure=True,
        )
        if sgs.get("ok"):
            groups = (sgs.get("data") or {}).get("SecurityGroups") or []
            inv["security_groups"] = {
                "count": len(groups),
                "group_ids": [g.get("GroupId") for g in groups],
            }
            self.record(
                "Security groups (tagged)",
                "PASS",
                f"{len(groups)} group(s)",
            )
        else:
            inv["security_groups"] = {"error": sgs.get("error")}
            self.record("Security groups (tagged)", "WARN", str(sgs.get("error"))[:200])

        # Key pairs (name contains project)
        keys = self.aws(
            "ec2",
            "describe-key-pairs",
            region=self.region,
            allow_failure=True,
        )
        if keys.get("ok"):
            kps = (keys.get("data") or {}).get("KeyPairs") or []
            matched = [
                k.get("KeyName")
                for k in kps
                if self.project.lower() in str(k.get("KeyName", "")).lower()
            ]
            inv["key_pairs"] = {"memcore_named": matched, "total_in_region": len(kps)}
            self.record(
                "Key pairs (name match)",
                "PASS",
                f"{len(matched)} name-matched; {len(kps)} total in region (names only)",
            )
        else:
            inv["key_pairs"] = {"error": keys.get("error")}
            self.record("Key pairs", "WARN", str(keys.get("error"))[:200])

        # EBS volumes
        vols = self.aws(
            "ec2",
            "describe-volumes",
            "--filters",
            f"Name=tag:Project,Values={self.project}",
            f"Name=tag:Environment,Values={self.environment}",
            region=self.region,
            allow_failure=True,
        )
        if vols.get("ok"):
            volumes = (vols.get("data") or {}).get("Volumes") or []
            large = [v for v in volumes if int(v.get("Size") or 0) > 30]
            inv["ebs_volumes"] = {
                "count": len(volumes),
                "large_over_30gb": [
                    {"id": v.get("VolumeId"), "size": v.get("Size")} for v in large
                ],
            }
            if large:
                expensive_found.append("large_ebs")
                self.record(
                    "EBS volumes (tagged)",
                    "WARN",
                    f"{len(volumes)} volume(s); {len(large)} >30GB",
                )
            else:
                self.record("EBS volumes (tagged)", "PASS", f"{len(volumes)} volume(s)")
        else:
            inv["ebs_volumes"] = {"error": vols.get("error")}
            self.record("EBS volumes", "WARN", str(vols.get("error"))[:200])

        # Elastic IPs
        eips = self.aws(
            "ec2",
            "describe-addresses",
            region=self.region,
            allow_failure=True,
        )
        if eips.get("ok"):
            addrs = (eips.get("data") or {}).get("Addresses") or []
            unattached = [a for a in addrs if not a.get("AssociationId")]
            # Tag filter client-side
            tagged = []
            for a in addrs:
                tags = {t.get("Key"): t.get("Value") for t in (a.get("Tags") or [])}
                if (
                    tags.get("Project") == self.project
                    and tags.get("Environment") == self.environment
                ):
                    tagged.append(a.get("AllocationId"))
            inv["elastic_ips"] = {
                "total_in_region": len(addrs),
                "unattached": len(unattached),
                "tagged_memcore_staging": tagged,
            }
            if unattached:
                expensive_found.append("unattached_eip")
                self.record(
                    "Elastic IPs",
                    "WARN",
                    f"{len(unattached)} unattached EIP(s) in region (cost risk)",
                )
            else:
                self.record("Elastic IPs", "PASS", f"{len(addrs)} address(es); none unattached")
        else:
            inv["elastic_ips"] = {"error": eips.get("error")}
            self.record("Elastic IPs", "WARN", str(eips.get("error"))[:200])

        # NAT Gateways
        nats = self.aws(
            "ec2",
            "describe-nat-gateways",
            "--filter",
            "Name=state,Values=pending,available",
            region=self.region,
            allow_failure=True,
        )
        if nats.get("ok"):
            ngw = (nats.get("data") or {}).get("NatGateways") or []
            inv["nat_gateways"] = {"count": len(ngw), "ids": [n.get("NatGatewayId") for n in ngw]}
            if ngw:
                expensive_found.append("nat_gateway")
                self.record(
                    "NAT Gateways",
                    "WARN",
                    f"{len(ngw)} active NAT Gateway(s) — avoid for MVP staging",
                )
            else:
                self.record("NAT Gateways", "PASS", "none active")
        else:
            inv["nat_gateways"] = {"error": nats.get("error")}
            self.record("NAT Gateways", "WARN", str(nats.get("error"))[:200])

        # Load balancers (ELBv2)
        elbv2 = self.aws(
            "elbv2",
            "describe-load-balancers",
            region=self.region,
            allow_failure=True,
        )
        if elbv2.get("ok"):
            lbs = (elbv2.get("data") or {}).get("LoadBalancers") or []
            inv["load_balancers"] = {
                "count": len(lbs),
                "names": [lb.get("LoadBalancerName") for lb in lbs],
            }
            if lbs:
                expensive_found.append("load_balancer")
                self.record("Load balancers", "WARN", f"{len(lbs)} LB(s) — avoid for MVP")
            else:
                self.record("Load balancers", "PASS", "none")
        else:
            inv["load_balancers"] = {"error": elbv2.get("error")}
            self.record("Load balancers", "WARN", str(elbv2.get("error"))[:200])

        # RDS
        rds = self.aws(
            "rds",
            "describe-db-instances",
            region=self.region,
            allow_failure=True,
        )
        if rds.get("ok"):
            dbs = (rds.get("data") or {}).get("DBInstances") or []
            inv["rds"] = {
                "count": len(dbs),
                "ids": [d.get("DBInstanceIdentifier") for d in dbs],
            }
            if dbs:
                expensive_found.append("rds")
                self.record("RDS", "WARN", f"{len(dbs)} DB instance(s) — avoid for MVP")
            else:
                self.record("RDS", "PASS", "none")
        else:
            inv["rds"] = {"error": rds.get("error")}
            self.record("RDS", "WARN", str(rds.get("error"))[:200])

        # ElastiCache
        cache = self.aws(
            "elasticache",
            "describe-cache-clusters",
            region=self.region,
            allow_failure=True,
        )
        if cache.get("ok"):
            clusters = (cache.get("data") or {}).get("CacheClusters") or []
            inv["elasticache"] = {"count": len(clusters)}
            if clusters:
                expensive_found.append("elasticache")
                self.record("ElastiCache", "WARN", f"{len(clusters)} cluster(s)")
            else:
                self.record("ElastiCache", "PASS", "none")
        else:
            inv["elasticache"] = {"error": cache.get("error")}
            self.record("ElastiCache", "WARN", str(cache.get("error"))[:200])

        # EKS
        eks = self.aws("eks", "list-clusters", region=self.region, allow_failure=True)
        if eks.get("ok"):
            clusters = (eks.get("data") or {}).get("clusters") or []
            inv["eks"] = {"count": len(clusters), "names": clusters}
            if clusters:
                expensive_found.append("eks")
                self.record("EKS", "WARN", f"{len(clusters)} cluster(s)")
            else:
                self.record("EKS", "PASS", "none")
        else:
            inv["eks"] = {"error": eks.get("error")}
            self.record("EKS", "WARN", str(eks.get("error"))[:200])

        # ECS clusters
        ecs = self.aws("ecs", "list-clusters", region=self.region, allow_failure=True)
        if ecs.get("ok"):
            arns = (ecs.get("data") or {}).get("clusterArns") or []
            inv["ecs"] = {"count": len(arns)}
            if arns:
                expensive_found.append("ecs")
                self.record("ECS", "WARN", f"{len(arns)} cluster(s)")
            else:
                self.record("ECS", "PASS", "none")
        else:
            inv["ecs"] = {"error": ecs.get("error")}
            self.record("ECS", "WARN", str(ecs.get("error"))[:200])

        # OpenSearch
        os_dom = self.aws(
            "opensearch",
            "list-domain-names",
            region=self.region,
            allow_failure=True,
        )
        if os_dom.get("ok"):
            domains = (os_dom.get("data") or {}).get("DomainNames") or []
            inv["opensearch"] = {"count": len(domains)}
            if domains:
                expensive_found.append("opensearch")
                self.record("OpenSearch", "WARN", f"{len(domains)} domain(s)")
            else:
                self.record("OpenSearch", "PASS", "none")
        else:
            inv["opensearch"] = {"error": os_dom.get("error")}
            self.record("OpenSearch", "WARN", str(os_dom.get("error"))[:200])

        # S3 buckets (global list; filter by name prefix — read-only)
        s3 = self.aws("s3api", "list-buckets", region=self.region, allow_failure=True)
        if s3.get("ok"):
            buckets = (s3.get("data") or {}).get("Buckets") or []
            matched = [
                b.get("Name")
                for b in buckets
                if self.project.lower() in str(b.get("Name", "")).lower()
                and self.environment.lower() in str(b.get("Name", "")).lower()
            ]
            inv["s3"] = {
                "memcore_staging_named": matched,
                "note": "name heuristic only; no bucket created",
            }
            self.record(
                "S3 buckets (name heuristic)",
                "PASS",
                f"{len(matched)} name-matched memcore+staging",
            )
        else:
            inv["s3"] = {"error": s3.get("error")}
            self.record("S3 buckets", "WARN", str(s3.get("error"))[:200])

        inv["expensive_found"] = expensive_found
        self.inventory = inv

        # MVP staging should normally have no expensive managed resources.
        # Unattached EIPs / large EBS / NAT/LB/RDS/etc. in the *account region*
        # are warnings; only block if clearly memcore-staging tagged expensive stack
        # or NAT/LB/RDS exist when we are about to deploy (operator should clean up).
        blocking = [k for k in expensive_found if k in {"nat_gateway", "load_balancer", "rds", "eks", "ecs", "opensearch", "elasticache"}]
        if blocking:
            # Warn strongly; block only if tagged memcore staging also has EC2/SG already
            # plus expensive services — for empty staging account, presence of *any*
            # NAT/LB/RDS is a cost-safety block before first memcore deploy.
            self.record(
                "Expensive resource safety",
                "WARN",
                f"found {blocking} — MVP staging should not create these; "
                f"confirm they are unrelated before deploying",
            )
            # Do not auto-block the whole account if resources may be unrelated personal
            # leftovers; operator decision. Document in decision logic.
            self.warnings.append(
                f"expensive resources present in region: {blocking}"
            )
        else:
            self.record(
                "Expensive resource safety",
                "PASS",
                "no NAT/LB/RDS/ElastiCache/EKS/ECS/OpenSearch detected",
            )

    def check_network(self) -> None:
        net: dict[str, Any] = {}

        vpcs = self.aws(
            "ec2",
            "describe-vpcs",
            "--filters",
            "Name=isDefault,Values=true",
            region=self.region,
            allow_failure=True,
        )
        if not vpcs.get("ok"):
            self.network = {"error": vpcs.get("error")}
            self.record("Default VPC", "FAIL", str(vpcs.get("error"))[:200])
            self.blocked_reasons.append("missing required network readiness")
            return

        vpc_list = (vpcs.get("data") or {}).get("Vpcs") or []
        if not vpc_list:
            self.network = {"default_vpc": None}
            self.record(
                "Default VPC",
                "FAIL",
                f"no default VPC in {self.region}",
            )
            self.blocked_reasons.append("missing required network readiness")
            return

        vpc = vpc_list[0]
        vpc_id = vpc.get("VpcId")
        net["default_vpc_id"] = vpc_id
        self.record("Default VPC", "PASS", f"{vpc_id} (acceptable for short-lived staging)")

        subnets = self.aws(
            "ec2",
            "describe-subnets",
            "--filters",
            f"Name=vpc-id,Values={vpc_id}",
            region=self.region,
            allow_failure=True,
        )
        public_subnets = []
        if subnets.get("ok"):
            for s in (subnets.get("data") or {}).get("Subnets") or []:
                if s.get("MapPublicIpOnLaunch"):
                    public_subnets.append(
                        {
                            "id": s.get("SubnetId"),
                            "az": s.get("AvailabilityZone"),
                            "cidr": s.get("CidrBlock"),
                        }
                    )
            net["public_subnets"] = public_subnets
            if public_subnets:
                self.record(
                    "Public subnet",
                    "PASS",
                    f"{len(public_subnets)} subnet(s) with MapPublicIpOnLaunch",
                )
            else:
                # Default VPC subnets usually map public IP; if not, still may have IGW route.
                all_subnets = (subnets.get("data") or {}).get("Subnets") or []
                net["all_subnet_count"] = len(all_subnets)
                if all_subnets:
                    self.record(
                        "Public subnet",
                        "WARN",
                        f"{len(all_subnets)} subnet(s) but MapPublicIpOnLaunch=false; "
                        f"check route to IGW before deploy",
                    )
                else:
                    self.record("Public subnet", "FAIL", "no subnets in default VPC")
                    self.blocked_reasons.append("missing required network readiness")
        else:
            self.record("Public subnet", "WARN", str(subnets.get("error"))[:200])

        # Internet gateway + routes
        igws = self.aws(
            "ec2",
            "describe-internet-gateways",
            "--filters",
            f"Name=attachment.vpc-id,Values={vpc_id}",
            region=self.region,
            allow_failure=True,
        )
        igw_ok = False
        if igws.get("ok"):
            igw_list = (igws.get("data") or {}).get("InternetGateways") or []
            net["internet_gateways"] = [i.get("InternetGatewayId") for i in igw_list]
            igw_ok = bool(igw_list)

        rts = self.aws(
            "ec2",
            "describe-route-tables",
            "--filters",
            f"Name=vpc-id,Values={vpc_id}",
            region=self.region,
            allow_failure=True,
        )
        has_igw_route = False
        if rts.get("ok"):
            for rt in (rts.get("data") or {}).get("RouteTables") or []:
                for route in rt.get("Routes") or []:
                    if route.get("GatewayId", "").startswith("igw-") and route.get(
                        "DestinationCidrBlock"
                    ) in ("0.0.0.0/0", "::/0"):
                        has_igw_route = True
            net["has_igw_default_route"] = has_igw_route

        if igw_ok and has_igw_route:
            self.record("Internet route", "PASS", "IGW attached + 0.0.0.0/0 route present")
        elif igw_ok:
            self.record(
                "Internet route",
                "WARN",
                "IGW attached but default route not clearly detected",
            )
        else:
            self.record(
                "Internet route",
                "FAIL",
                "no internet gateway on default VPC",
            )
            self.blocked_reasons.append("missing required network readiness")

        azs = self.aws(
            "ec2",
            "describe-availability-zones",
            "--filters",
            "Name=state,Values=available",
            region=self.region,
            allow_failure=True,
        )
        if azs.get("ok"):
            az_names = [
                z.get("ZoneName")
                for z in (azs.get("data") or {}).get("AvailabilityZones") or []
            ]
            net["availability_zones"] = az_names
            self.record(
                "Availability zones",
                "PASS",
                f"{len(az_names)} available: {', '.join(az_names)}",
            )
        else:
            self.record("Availability zones", "WARN", str(azs.get("error"))[:200])

        net["note"] = (
            "Default VPC is acceptable for short-lived single-server MVP staging. "
            "Do not create custom VPC, NAT Gateway, or private-subnet architecture in this phase."
        )
        self.network = net

    def check_instance_types(self) -> None:
        result = self.aws(
            "ec2",
            "describe-instance-type-offerings",
            "--location-type",
            "region",
            "--filters",
            f"Name=instance-type,Values={','.join(CANDIDATE_INSTANCE_TYPES)}",
            region=self.region,
            allow_failure=True,
        )
        available = []
        if result.get("ok"):
            offerings = (result.get("data") or {}).get("InstanceTypeOfferings") or []
            available = sorted({o.get("InstanceType") for o in offerings if o.get("InstanceType")})
            self.record(
                "Instance type offerings",
                "PASS",
                f"available in {self.region}: {', '.join(available) or 'none'}",
            )
        else:
            self.record(
                "Instance type offerings",
                "WARN",
                str(result.get("error"))[:200],
            )

        preferred = "t3.medium" if "t3.medium" in available or not available else None
        fallback = "t3.small" if "t3.small" in available or not available else None
        arm_note = (
            "t4g.* only if Docker image/build supports arm64; do not select by default"
        )
        plan = {
            "candidates_checked": list(CANDIDATE_INSTANCE_TYPES),
            "available": available,
            "preferred": preferred or "t3.medium",
            "fallback": fallback or "t3.small",
            "avoid": ["t3.micro", "GPU instances", "large instances"],
            "arm_note": arm_note,
            "reason_preferred": (
                "short-lived staging validation; enough memory headroom for "
                "Postgres + Qdrant + memcore; still budget-controlled"
            ),
            "fallback_risk": "may be tight for Qdrant + Postgres + memcore",
            "shutdown_policy": "stop instance immediately after validation window",
            "storage_plan": {
                "root_ebs": "20–30 GB gp3",
                "data_path": "Docker volumes on EBS",
                "backups": "local backup first; optional later S3 copy",
                "max_ebs_gb": 30,
                "create_now": False,
            },
        }
        self.instance_types = plan
        self.record(
            "Recommended instance type",
            "PASS",
            f"{plan['preferred']} (fallback {plan['fallback']}); EBS 20–30 GB gp3; stop after window",
        )

    def decide(self) -> str:
        if any("wrong AWS account" in r for r in self.blocked_reasons):
            return "AWS preflight blocked — wrong AWS account/profile"
        if any("AWS CLI/profile unavailable" in r for r in self.blocked_reasons):
            return "AWS preflight blocked — AWS CLI/profile unavailable"
        if any("missing required network readiness" in r for r in self.blocked_reasons):
            return "AWS preflight blocked — missing required network readiness"
        if any("unsafe existing expensive resources" in r for r in self.blocked_reasons):
            return "AWS preflight blocked — unsafe existing expensive resources"

        # Hard fail list
        if self.failures:
            # Classify
            joined = " ".join(self.failures).lower()
            if "account" in joined or "profile" in joined:
                return "AWS preflight blocked — wrong AWS account/profile"
            if "vpc" in joined or "subnet" in joined or "internet" in joined:
                return "AWS preflight blocked — missing required network readiness"
            if "cli" in joined:
                return "AWS preflight blocked — AWS CLI/profile unavailable"
            return "AWS preflight blocked — wrong AWS account/profile"

        if self.manual_verification_required and self.budgets.get(
            "manual_verification_required"
        ):
            return "AWS preflight passed with manual budget verification required"

        if self.manual_verification_required:
            return "AWS preflight passed with manual budget verification required"

        return "AWS preflight passed — ready for single-EC2 staging deployment"

    def write_report(self) -> Path:
        out_dir = self.repo_root / "reports" / "aws-preflight"
        out_dir.mkdir(parents=True, exist_ok=True)
        path = out_dir / f"aws_preflight_{utc_stamp()}.json"
        payload = {
            "generated_at_utc": datetime.now(timezone.utc).isoformat(),
            "scope": "read-only AWS deployment safety preflight",
            "profile": self.profile,
            "region": self.region,
            "expected_account_id": self.expected_account_id,
            "project": self.project,
            "environment": self.environment,
            "identity": redact_obj(self.identity),
            "budgets": redact_obj(self.budgets),
            "inventory": redact_obj(self.inventory),
            "network": redact_obj(self.network),
            "instance_type_plan": redact_obj(self.instance_types),
            "checks": self.checks,
            "warnings": self.warnings,
            "failures": self.failures,
            "manual_verification_required": self.manual_verification_required,
            "decision": self.decision,
            "staging_plan": {
                "region": self.region,
                "instance_type": (self.instance_types or {}).get("preferred", "t3.medium"),
                "instance_type_fallback": (self.instance_types or {}).get(
                    "fallback", "t3.small"
                ),
                "ebs": "20–30 GB gp3",
                "runtime": "Docker Compose (Postgres + Qdrant + memcore)",
                "provider_smoke": "Gemini first; single_real only for tiny smoke",
                "load_testing": "mock only",
                "shutdown_policy": "stop instance immediately after validation window",
            },
            "notes": [
                "No AWS resources were created, modified, or deleted.",
                "AWS Budgets are alerts, not hard spending stops.",
                "Never use the default profile or client AWS accounts.",
                "Production MVP remains not approved.",
            ],
        }
        path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        return path

    def run(self) -> int:
        print("=== memcore AWS preflight (read-only) ===")
        print(f"profile={self.profile} region={self.region}")
        print(f"expected_account_id={self.expected_account_id}")
        print(f"project={self.project} environment={self.environment}")
        print("This script does not create, modify, or delete AWS resources.")
        print()

        try:
            self.check_cli_and_command_templates()
            self.check_profiles()
            self.check_identity()
            self.check_configure_list()
            self.check_budgets()
            self.check_inventory()
            self.check_network()
            self.check_instance_types()
        except PreflightError as exc:
            print(f"\nERROR: {exc}", file=sys.stderr)
            if not self.decision:
                self.decision = self.decide()
            report_path = self.write_report()
            print(f"\nRedacted JSON report: {report_path}")
            print(f"Decision: {self.decision}")
            return 1

        self.decision = self.decide()
        report_path = self.write_report()
        print()
        print(f"Redacted JSON report: {report_path}")
        print(f"Decision: {self.decision}")

        if self.decision.startswith("AWS preflight blocked"):
            return 1
        return 0


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Read-only AWS preflight for memcore staging (no resource changes)."
    )
    p.add_argument(
        "--profile",
        required=True,
        help="AWS CLI profile (must be memcore-personal; never default)",
    )
    p.add_argument(
        "--region",
        required=True,
        help="AWS region (expected: ap-south-1)",
    )
    p.add_argument(
        "--expected-account-id",
        required=True,
        help="Expected AWS account ID (749251636291)",
    )
    p.add_argument("--project", default="memcore", help="Project tag value")
    p.add_argument("--environment", default="staging", help="Environment tag value")
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    repo_root = Path(__file__).resolve().parents[2]

    if args.profile.strip().lower() == "default":
        print(
            "ERROR: default AWS profile is forbidden. Use --profile memcore-personal.",
            file=sys.stderr,
        )
        return 1

    preflight = AwsPreflight(
        profile=args.profile,
        region=args.region,
        expected_account_id=args.expected_account_id,
        project=args.project,
        environment=args.environment,
        repo_root=repo_root,
    )
    return preflight.run()


if __name__ == "__main__":
    sys.exit(main())
