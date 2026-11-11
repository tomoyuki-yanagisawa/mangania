use mangania::config::Config;

#[test]
fn parses_spread_from_toml() {
    assert!(Config::parse("spread = true\n").spread);
    assert!(!Config::parse("# comment\nspread = false\n").spread);
    assert!(!Config::parse("spread = nope\n").spread);
    assert!(!Config::parse("").spread);
}

#[test]
fn round_trips_config_file() {
    let dir = std::env::temp_dir().join(format!("mangania-config-{}", std::process::id()));
    let path = dir.join("config.toml");
    let config = Config { spread: true };

    config.save_to(&path).unwrap();
    assert!(Config::load_from(&path).spread);

    std::fs::remove_dir_all(&dir).unwrap();
}
