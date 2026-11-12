use std::path::PathBuf;

use mangania::config::Config;

#[test]
fn parses_spread_and_export_settings() {
    assert!(Config::parse("spread = true\n").spread);
    assert!(!Config::parse("# comment\nspread = false\n").spread);
    assert!(!Config::parse("spread = nope\n").spread);
    assert!(!Config::parse("").spread);

    let config = Config::parse(
        "spread = true\nexport_dir = \"/tmp/mangania-out\"\nexport_name = \"{book}_{page:03}\"\n",
    );
    assert!(config.spread);
    assert_eq!(config.export_dir, Some(PathBuf::from("/tmp/mangania-out")));
    assert_eq!(config.export_name, "{book}_{page:03}");
}

#[test]
fn round_trips_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-{}", std::process::id()));
    let path = dir.join("config.toml");
    let config = Config {
        spread: true,
        export_dir: Some(PathBuf::from("/tmp/mangania-out")),
        export_name: "{book}_{page:03}_{name}".to_string(),
    };

    config.save_to(&path).unwrap();
    assert_eq!(Config::load_from(&path), config);

    std::fs::remove_dir_all(&dir).unwrap();
}
