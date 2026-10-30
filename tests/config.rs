// tests/config.rs — 阶段一: 默认加载, 校验, bind 解析 (§5.2)
use story_architect::config;

#[test]
fn defaults() {
    let cfg = match config::load() {
        Err(e) => panic!("config load failed: {e}", e = e),
        Ok(c) => c,
    };
    if cfg.bind != "127.0.0.1:8080" {
        panic!("bind default wrong: {b}", b = cfg.bind);
    }
    if cfg.http.dev_cors {
        panic!("http.dev_cors must default false (production deny-all, D-28)");
    }
    if cfg.fts_min_query_length != 2 {
        panic!("fts_min_query_length must be 2 (appendix B)");
    }
    if cfg.http.import_body_limit_bytes != 67108864 {
        panic!("import body limit must be 64MB (D-04)");
    }
}

#[test]
fn bind_parsing() {
    let good = match config::parse_bind("127.0.0.1:8080".to_string()) {
        Err(_) => false,
        Ok(a) => a.port == 8080 && a.host == "127.0.0.1",
    };
    if !good {
        panic!("parse_bind 127.0.0.1:8080 must ok")
    }
    let bad = config::parse_bind("8080".to_string()).is_err();
    if !bad {
        panic!("bind without host must fail")
    }
    let zero = config::parse_bind("127.0.0.1:0".to_string()).is_err();
    if !zero {
        panic!("bind port 0 must fail (section 5.2)")
    }
}

#[test]
fn effective_data_dir_is_shared_single_implementation() {
    // 显式配置优先
    let explicit = config::effective_data_dir("/tmp/sf-explicit");
    assert_eq!(explicit.display().to_string(), "/tmp/sf-explicit");

    // 空白配置回退平台默认
    let fallback = config::effective_data_dir("   ");
    assert!(fallback.is_absolute(), "默认数据目录必须是绝对路径");

    #[cfg(target_os = "macos")]
    {
        assert!(
            fallback
                .display()
                .to_string()
                .ends_with("Library/Application Support/StoryArchitect"),
            "macOS 默认目录必须是 ~/Library/Application Support/StoryArchitect，实际 {p}",
            p = fallback.display()
        );
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let s = fallback.display().to_string();
        assert!(
            s.ends_with("story-architect"),
            "unix 默认目录必须以 story-architect 结尾，实际 {s}"
        );
    }
}
