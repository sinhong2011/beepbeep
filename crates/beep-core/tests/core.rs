// SPDX-License-Identifier: AGPL-3.0-or-later
use beep_core::domain::{Platform, Segment};
use beep_core::engine::{RateLimiter, disposition_filename, plan, split_largest};
use beep_core::providers::{classify, instagram_code, parse_meta_page, parse_syndication, tweet_id, x_token};
use beep_core::settings::Quality;
use beep_core::util::{extract_links, redact, sanitize, unique_path};
use std::collections::HashSet;

#[test]
fn plan_covers_file_without_gaps() {
    for length in [1u64, 1000, 1 << 20, (1 << 20) * 8 + 7, 5_000_000_123] {
        for conns in [1, 3, 8, 32] {
            let segs = plan(length, conns, 1 << 20);
            assert_eq!(segs[0].start, 0);
            assert_eq!(segs.last().unwrap().end, length - 1);
            for w in segs.windows(2) {
                assert_eq!(w[0].end + 1, w[1].start);
            }
            assert_eq!(segs.iter().map(Segment::len).sum::<u64>(), length);
            assert!(segs.len() <= conns);
        }
    }
    assert_eq!(plan(3 << 20, 16, 1 << 20).len(), 3);
    assert_eq!(plan(1000, 16, 1 << 20).len(), 1);
}

#[test]
fn split_keeps_coverage() {
    let mut segs = plan(10 << 20, 2, 1 << 20);
    segs[0].received = segs[0].len();
    segs[1].received = 1 << 20;
    let before = segs[1];
    assert_eq!(split_largest(&mut segs, 1 << 20), Some(2));
    assert_eq!(segs[1].end + 1, segs[2].start);
    assert_eq!(segs[2].end, before.end);
    assert!(segs[1].end > segs[1].cursor());
    let mut tiny = vec![Segment::new(0, 100)];
    assert_eq!(split_largest(&mut tiny, 1 << 20), None);
}

#[test]
fn classifies_post_links() {
    let cases = [
        ("https://x.com/nasa/status/1585341984679469056", Some(Platform::X)),
        ("https://twitter.com/nasa/status/1585341984679469056?s=20", Some(Platform::X)),
        ("https://mobile.twitter.com/i/status/123/video/1", Some(Platform::X)),
        ("https://fxtwitter.com/a/status/42", Some(Platform::X)),
        ("https://x.com/nasa", None),
        ("https://www.threads.net/@zuck/post/C8Hb6jZv0Hy", Some(Platform::Threads)),
        ("https://www.threads.com/@zuck/post/C8Hb6jZv0Hy?xmt=1", Some(Platform::Threads)),
        ("https://www.instagram.com/reel/C0Yx1b5x0kX/?igsh=abc", Some(Platform::Instagram)),
        ("https://instagram.com/p/C0Yx1b5x0kX/", Some(Platform::Instagram)),
        ("https://www.instagram.com/nasa/", None),
        ("https://example.com/file.zip", None),
    ];
    for (u, p) in cases {
        assert_eq!(classify(u), p, "{u}");
    }
    assert_eq!(tweet_id(&url::Url::parse("https://x.com/a/status/99/photo/1").unwrap()).as_deref(), Some("99"));
    assert_eq!(instagram_code(&url::Url::parse("https://www.instagram.com/reels/ABC_-1/").unwrap()).as_deref(), Some("ABC_-1"));
}

#[test]
fn x_token_matches_javascript() {
    // ((id/1e15)*Math.PI).toString(36).replace(/(0+|\.)/g,'') evaluated in Node.
    assert_eq!(x_token("1585341984679469056"), "3uchycv2wqc");
    assert_eq!(x_token("20"), "6dq1a2xwd93");
    assert_eq!(x_token("1790000000000000001"), "4c7g8auqyik");
    assert_eq!(x_token("1234567890123456789"), "2zqic77uqyk");
}

#[test]
fn x_syndication_parses_variants() {
    let json = serde_json::json!({
        "text": "Hello https://t.co/abc",
        "user": {"screen_name": "nasa", "name": "NASA"},
        "mediaDetails": [{
            "type": "video",
            "media_url_https": "https://pbs.twimg.com/thumb.jpg",
            "video_info": {"duration_millis": 12000, "variants": [
                {"content_type": "application/x-mpegURL", "url": "https://video.twimg.com/pl/x.m3u8"},
                {"bitrate": 256000, "content_type": "video/mp4", "url": "https://video.twimg.com/vid/480x270/a.mp4?tag=14"},
                {"bitrate": 2176000, "content_type": "video/mp4", "url": "https://video.twimg.com/vid/1280x720/b.mp4?tag=14"},
                {"bitrate": 10368000, "content_type": "video/mp4", "url": "https://video.twimg.com/vid/1920x1080/c.mp4?tag=14"}
            ]}
        }]
    });
    let items = parse_syndication(&json, "https://x.com/i/status/1", "1").unwrap();
    assert_eq!(items.len(), 1);
    let i = &items[0];
    assert_eq!(i.title.as_deref(), Some("Hello"));
    assert_eq!(i.author.as_deref(), Some("@nasa"));
    assert_eq!(i.duration, Some(12.0));
    assert_eq!(i.formats.len(), 3);
    assert_eq!(i.pick(Quality::Best).unwrap().short_side(), 1080);
    assert_eq!(i.pick(Quality::P720).unwrap().short_side(), 720);
    assert_eq!(i.pick(Quality::P480).unwrap().short_side(), 270);
    let none = serde_json::json!({"text": "hi", "mediaDetails": [{"type": "photo"}]});
    assert!(parse_syndication(&none, "https://x.com/i/status/1", "1").is_err());
}

#[test]
fn meta_page_video_versions() {
    let html = r#"<html><head>
        <meta property="og:title" content="Mark Zuckerberg (@zuck) on Threads">
        <meta property="og:description" content="Big news &amp; more">
        <meta property="og:image" content="https://scontent.cdninstagram.com/thumb.jpg?x=1&amp;y=2">
        </head><body><script type="application/json">
        {"post":{"video_versions":[{"type":101,"width":720,"height":1280,"url":"https:\/\/scontent.cdninstagram.com\/v\/a.mp4?oh=1&oe=2"},
        {"type":102,"width":480,"height":854,"url":"https:\/\/scontent.cdninstagram.com\/v\/b.mp4?oh=1"}]}}
        </script></body></html>"#;
    let p = parse_meta_page(html);
    assert_eq!(p.videos.len(), 2);
    assert_eq!(p.videos[0].url.as_deref(), Some("https://scontent.cdninstagram.com/v/a.mp4?oh=1&oe=2"));
    assert_eq!(p.videos[0].short_side(), 720);
    assert_eq!(p.author.as_deref(), Some("@zuck"));
    assert_eq!(p.title.as_deref(), Some("Big news & more"));
    assert_eq!(p.thumbnail.as_deref(), Some("https://scontent.cdninstagram.com/thumb.jpg?x=1&y=2"));

    let og = r#"<meta property="og:video" content="https://cdn.example.com/v.mp4"><meta property="og:video:width" content="1080"><meta property="og:video:height" content="1920">"#;
    let p = parse_meta_page(og);
    assert_eq!(p.videos.len(), 1);
    assert_eq!(p.videos[0].short_side(), 1080);
}

#[test]
fn link_collector() {
    let text = "check https://x.com/a/status/1 and\nhttps://www.threads.net/@b/post/XYZ, also (https://example.com/a.zip) dup https://x.com/a/status/1.";
    assert_eq!(
        extract_links(text),
        vec!["https://x.com/a/status/1", "https://www.threads.net/@b/post/XYZ", "https://example.com/a.zip"]
    );
}

#[test]
fn file_safety() {
    assert_eq!(sanitize("a/b:c?.mp4", 180), "a b c .mp4");
    assert_eq!(sanitize("...", 180), "download");
    let long = "x".repeat(300) + ".mp4";
    let s = sanitize(&long, 180);
    assert!(s.ends_with(".mp4") && s.chars().count() <= 180);

    let dir = std::env::temp_dir().join(format!("beep-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("v.mp4"), b"").unwrap();
    let none = HashSet::new();
    assert_eq!(unique_path(&dir, "v.mp4", &none, true).file_name().unwrap(), "v (1).mp4");
    let reserved: HashSet<String> = ["w.mp4".to_string()].into();
    assert_eq!(unique_path(&dir, "w.mp4", &reserved, true).file_name().unwrap(), "w (1).mp4");
    std::fs::write(dir.join("z.mp4.beeppart"), b"").unwrap();
    assert_eq!(unique_path(&dir, "z.mp4", &none, true).file_name().unwrap(), "z (1).mp4");
    assert_eq!(unique_path(&dir, "z.mp4", &none, false).file_name().unwrap(), "z.mp4");
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn redaction_hides_signatures() {
    let s = redact("GET https://cdn.example.com/v.mp4?oh=SECRET&oe=123&tag=14 done");
    assert!(!s.contains("SECRET") && s.contains("tag=14"));
}

#[test]
fn content_disposition() {
    assert_eq!(disposition_filename(r#"attachment; filename="a b.zip""#).as_deref(), Some("a b.zip"));
    assert_eq!(disposition_filename("attachment; filename*=UTF-8''%E2%82%AC%20rates.pdf").as_deref(), Some("€ rates.pdf"));
}

#[test]
fn rate_limiter() {
    assert!(RateLimiter::new(0).take(10_000_000).is_zero());
    let r = RateLimiter::new(1000);
    assert!(r.take(1000).as_secs_f64() < 0.01);
    assert!((r.take(1000).as_secs_f64() - 1.0).abs() < 0.05);
}
