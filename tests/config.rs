use std::path::PathBuf;

use mangania::config::{Config, DEFAULT_CACHE_MB, DEFAULT_PREFETCH};

#[test]
fn parses_all_config_fields() {
    assert!(Config::parse("spread = true\n").spread);
    assert!(!Config::parse("# comment\nspread = false\n").spread);
    assert!(!Config::parse("spread = nope\n").spread);
    assert!(!Config::parse("").spread);

    let config = Config::parse(
        "spread = true\nexport_dir = \"/tmp/mangania-out\"\nexport_name = \"{book}_{page:03}\"\nprefetch = 4\ncache_mb = 64\n",
    );
    assert!(config.spread);
    assert_eq!(config.export_dir, Some(PathBuf::from("/tmp/mangania-out")));
    assert_eq!(config.export_name, "{book}_{page:03}");
    assert_eq!(config.prefetch, 4);
    assert_eq!(config.cache_mb, 64);
    assert_eq!(config.cache_bytes(), 64 * 1024 * 1024);
}

#[test]
fn uses_defaults_for_prefetch_and_cache() {
    let config = Config::default();
    assert_eq!(config.prefetch, DEFAULT_PREFETCH);
    assert_eq!(config.cache_mb, DEFAULT_CACHE_MB);
    assert_eq!(config.cache_bytes(), DEFAULT_CACHE_MB * 1024 * 1024);

    let empty = Config::parse("");
    assert_eq!(empty.prefetch, DEFAULT_PREFETCH);
    assert_eq!(empty.cache_mb, DEFAULT_CACHE_MB);

    let no_cache = Config::parse("cache_mb = 0\n");
    assert_eq!(no_cache.cache_bytes(), DEFAULT_CACHE_MB * 1024 * 1024);
}

#[test]
fn default_config_text_round_trips() {
    let text = Config::default().to_toml();
    assert!(text.contains("# MangaNia settings"));
    assert!(text.contains("# export_dir"));
    assert_eq!(Config::parse(&text), Config::default());
}

#[test]
fn writes_default_config_on_first_load() {
    let dir = std::env::temp_dir().join(format!("mangania-config-create-{}", std::process::id()));
    let path = dir.join("config.toml");

    let config = Config::load_or_create_from(&path);

    assert_eq!(config, Config::default());
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("prefetch = 2"));
    assert!(text.contains("cache_mb = 128"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn keeps_existing_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-existing-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "spread = true\n").unwrap();

    let config = Config::load_or_create_from(&path);

    assert!(config.spread);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "spread = true\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn round_trips_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-{}", std::process::id()));
    let path = dir.join("config.toml");
    let config = Config {
        spread: true,
        export_dir: Some(PathBuf::from("/tmp/mangania-out")),
        export_name: "{book}_{page:03}_{name}".to_string(),
        prefetch: 4,
        cache_mb: 64,
    };

    config.save_to(&path).unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("# MangaNia settings")
    );
    assert_eq!(Config::load_from(&path), config);

    std::fs::remove_dir_all(&dir).unwrap();
}
