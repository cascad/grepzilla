// path: crates/broker/src/config_tests.rs
#[cfg(test)]
mod tests {
    use broker::config::BrokerConfig;

    #[test]
    fn from_env_reads_all() {
        // изолируем текущий процесс максимально просто
        std::env::set_var("GZ_ADDR", "127.0.0.1:9999");
        std::env::set_var("GZ_WAL_DIR", "/tmp/walx");
        std::env::set_var("GZ_SEGMENTS_DIR", "/tmp/segx");
        std::env::set_var("GZ_PARALLELISM", "8");
        std::env::set_var("GZ_HOT_CAP", "1234");
        std::env::set_var("GZ_MANIFEST", "/tmp/manifest.json");
        std::env::set_var("GZ_SHARD", "77");

        let cfg = BrokerConfig::from_env();

        assert_eq!(cfg.addr, "127.0.0.1:9999");
        assert_eq!(cfg.wal_dir, "/tmp/walx");
        assert_eq!(cfg.segment_out_dir, "/tmp/segx");
        assert_eq!(cfg.parallelism, 8);
        assert_eq!(cfg.hot_cap, 1234);
        assert_eq!(cfg.manifest_path.as_deref(), Some("/tmp/manifest.json"));
        assert_eq!(cfg.shard, 77);

        // cleanup (по желанию)
        std::env::remove_var("GZ_ADDR");
        std::env::remove_var("GZ_WAL_DIR");
        std::env::remove_var("GZ_SEGMENTS_DIR");
        std::env::remove_var("GZ_PARALLELISM");
        std::env::remove_var("GZ_HOT_CAP");
        std::env::remove_var("GZ_MANIFEST");
        std::env::remove_var("GZ_SHARD");
    }
}
