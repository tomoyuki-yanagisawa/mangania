use std::path::PathBuf;

use mangania::config::{
    Config, DEFAULT_CACHE_MB, DEFAULT_CACHE_PAGES, DEFAULT_PREFETCH, DisplayMode, ScaleFilter,
};

#[test]
fn parses_all_config_fields() {
    assert_eq!(
        Config::parse("mode = \"spread\"\n").mode,
        DisplayMode::Spread
    );
    assert_eq!(
        Config::parse("# comment\nmode = \"single\"\n").mode,
        DisplayMode::Single
    );
    assert_eq!(
        Config::parse("mode = \"invalid\"\n").mode,
        DisplayMode::Single
    );
    assert_eq!(Config::parse("").mode, DisplayMode::Single);

    let config = Config::parse(
        "mode = \"mixed\"\nexport_dir = \"/tmp/mangania-out\"\nexport_name = \"{book}_{page:03}\"\nprefetch = 4\ncache_pages = 6\ncache_mb = 64\nscale_filter = \"lanczos3\"\ndebug = true\n",
    );
    assert_eq!(config.mode, DisplayMode::Mixed);
    assert_eq!(config.export_dir, Some(PathBuf::from("/tmp/mangania-out")));
    assert_eq!(config.export_name, "{book}_{page:03}");
    assert_eq!(config.prefetch, 4);
    assert_eq!(config.cache_pages, 6);
    assert_eq!(config.cache_mb, 64);
    assert_eq!(config.scale_filter, ScaleFilter::Lanczos3);
    assert!(config.debug);
    assert_eq!(config.cache_entries(), 6);
    assert_eq!(config.cache_bytes(), 64 * 1024 * 1024);
}

#[test]
fn uses_defaults_for_prefetch_and_cache() {
    let config = Config::default();
    assert_eq!(config.mode, DisplayMode::Single);
    assert_eq!(config.prefetch, DEFAULT_PREFETCH);
    assert_eq!(config.cache_pages, DEFAULT_CACHE_PAGES);
    assert_eq!(config.cache_entries(), DEFAULT_CACHE_PAGES);
    assert_eq!(config.cache_mb, DEFAULT_CACHE_MB);
    assert_eq!(config.cache_bytes(), DEFAULT_CACHE_MB * 1024 * 1024);
    let empty = Config::parse("");
    assert_eq!(empty.prefetch, DEFAULT_PREFETCH);
    assert_eq!(empty.cache_pages, DEFAULT_CACHE_PAGES);
    assert_eq!(empty.cache_mb, DEFAULT_CACHE_MB);

    let no_cache = Config::parse("cache_pages = 0\ncache_mb = 0\n");
    assert_eq!(no_cache.cache_entries(), DEFAULT_CACHE_PAGES);
    assert_eq!(no_cache.cache_bytes(), DEFAULT_CACHE_MB * 1024 * 1024);
}

#[test]
fn uses_triangle_when_scale_filter_is_missing_or_invalid() {
    assert_eq!(Config::default().scale_filter, ScaleFilter::Triangle);
    assert_eq!(Config::parse("").scale_filter, ScaleFilter::Triangle);
    assert_eq!(
        Config::parse("scale_filter = \"bogus\"\n").scale_filter,
        ScaleFilter::Triangle
    );

    let text = Config::default().to_toml();
    assert!(text.contains("scale_filter = \"triangle\""));
    assert!(text.contains("# scale_filter"));
}

#[test]
fn parses_every_scale_filter() {
    let filters = [
        ("nearest", ScaleFilter::Nearest),
        ("triangle", ScaleFilter::Triangle),
        ("hamming", ScaleFilter::Hamming),
        ("catmull_rom", ScaleFilter::CatmullRom),
        ("mitchell", ScaleFilter::Mitchell),
        ("gaussian", ScaleFilter::Gaussian),
        ("lanczos3", ScaleFilter::Lanczos3),
    ];
    for (name, expected) in filters {
        let config = Config::parse(&format!("scale_filter = \"{name}\"\n"));
        assert_eq!(config.scale_filter, expected, "scale_filter = {name}");
    }
    assert_eq!(ScaleFilter::ALL.len(), filters.len());

    for filter in ScaleFilter::ALL {
        let config = Config::parse(&format!("scale_filter = \"{}\"\n", filter.as_str()));
        assert_eq!(config.scale_filter, filter, "{}", filter.as_str());
    }
}

#[test]
fn default_config_text_round_trips() {
    let text = Config::default().to_toml();
    assert!(text.contains("# MangaNia settings"));
    assert!(text.contains("mode = \"single\""));
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
    assert!(text.contains("cache_pages = 8"));
    assert!(text.contains("cache_mb = 128"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn completes_existing_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-existing-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &path,
        "# keep this comment\nmode = \"spread\"\ncustom = \"keep\"\n\n[custom_table]\nvalue = 3\n",
    )
    .unwrap();

    let config = Config::load_or_create_from(&path);

    assert_eq!(config.mode, DisplayMode::Spread);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("# keep this comment"));
    assert!(text.contains("mode = \"spread\""));
    assert!(text.contains("custom = \"keep\""));
    assert!(text.contains("[custom_table]"));
    assert!(text.contains("value = 3"));
    assert!(text.contains("prefetch = 2"));
    assert!(text.contains("scale_filter = \"triangle\""));
    assert!(
        text.find("prefetch = 2").unwrap() < text.find("[custom_table]").unwrap(),
        "root keys must stay outside custom tables:\n{text}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn completion_is_idempotent() {
    let dir =
        std::env::temp_dir().join(format!("mangania-config-idempotent-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "mode = \"spread\"\n").unwrap();

    Config::load_or_create_from(&path);
    let first = std::fs::read_to_string(&path).unwrap();
    Config::load_or_create_from(&path);
    let second = std::fs::read_to_string(&path).unwrap();

    assert_eq!(first, second);
    assert!(second.contains("mode = \"spread\""));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn round_trips_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-{}", std::process::id()));
    let path = dir.join("config.toml");
    let config = Config {
        mode: DisplayMode::Mixed,
        export_dir: Some(PathBuf::from("/tmp/mangania-out")),
        export_name: "{book}_{page:03}_{name}".to_string(),
        prefetch: 4,
        cache_pages: 6,
        cache_mb: 64,
        scale_filter: ScaleFilter::Gaussian,
        debug: true,
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

#[test]
fn saves_display_mode_and_preserves_other_settings() {
    let dir = std::env::temp_dir().join(format!("mangania-config-state-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "mode = \"single\"\nprefetch = 7\n").unwrap();
    let mut config = Config::load_or_create_from(&path);
    config.mode = DisplayMode::Mixed;
    config.save_to(&path).unwrap();
    let loaded = Config::load_or_create_from(&path);
    assert_eq!(loaded.mode, DisplayMode::Mixed);
    assert_eq!(loaded.prefetch, 7);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_toml_is_not_overwritten() {
    let dir = std::env::temp_dir().join(format!("mangania-config-syntax-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "mode = \n").unwrap();
    let config = Config::load_or_create_from(&path);
    assert_eq!(config, Config::default());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "mode = \n");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_files_are_not_overwritten() {
    let dir = std::env::temp_dir().join(format!("mangania-config-invalid-{}", std::process::id()));
    let path = dir.join("config.toml");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(&path, "mode = \"invalid\"\n").unwrap();
    let config = Config::load_or_create_from(&path);
    assert_eq!(config, Config::default());
    assert!(config.save_to(&path).is_err());
    assert_eq!(
        std::fs::read_to_string(path).unwrap(),
        "mode = \"invalid\"\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
}
