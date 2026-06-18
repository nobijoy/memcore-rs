fn main() {
    #[cfg(feature = "lancedb")]
    {
        // LanceDB depends on crates that compile protobuf schemas at build time.
        let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc should exist");
        // SAFETY: PROTOC is read only by downstream build scripts in this cargo invocation.
        unsafe {
            std::env::set_var("PROTOC", protoc);
        }
    }
}
