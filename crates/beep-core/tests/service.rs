// SPDX-License-Identifier: AGPL-3.0-or-later
#![cfg(unix)]
use beep_core::{Role, Service, Settings};

#[test]
fn a_second_process_attaches_to_the_owner_and_shares_its_queue() {
    let dir = std::env::temp_dir().join(format!("beep-svc-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut settings = Settings::default();
    settings.download_dir = dir.join("dl");

    let owner = Service::connect_or_start(dir.clone(), Some(settings));
    assert!(matches!(owner.role(), Role::Owner));
    let client = Service::connect_or_start(dir.clone(), None);
    assert!(matches!(client.role(), Role::Client));

    // Added through the client, seen by the owner, and back through the client.
    let ids = client.add_urls(&["http://127.0.0.1:9/nothing.zip".into()], None);
    assert_eq!(ids.len(), 1);
    assert!(owner.snapshot().tasks.iter().any(|t| t.id == ids[0]));
    assert!(client.snapshot().tasks.iter().any(|t| t.id == ids[0]));

    // Commands and settings travel too.
    client.pause(ids[0]);
    client.set_setting("maxConcurrent", "5").unwrap();
    assert_eq!(owner.settings().max_concurrent, 5);
    assert!(client.set_setting("nonsense", "1").is_err());
    assert!(client.remove(ids[0], false).is_none());
    assert!(owner.snapshot().tasks.is_empty());

    // A version the client has seen yields nothing new.
    let v = client.snapshot().version;
    assert!(client.snapshot_since(v).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}
