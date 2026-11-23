use super::*;

fn temp_store() -> NotesStore {
    let dir = std::env::temp_dir().join(format!("daodian-notes-{}", new_id()));
    NotesStore::open(&dir).unwrap()
}

fn sample_doc(text: &str) -> Value {
    serde_json::json!({
        "type": "doc",
        "content": [{
            "type": "paragraph",
            "content": [{ "type": "text", "text": text }]
        }]
    })
}

#[test]
fn save_roundtrip_and_conflict() {
    let mut store = temp_store();
    let created = {
        let now = now_ms();
        let note = Note {
            id: "n_test".into(),
            title: "提测清单".into(),
            doc: sample_doc("登录和支付"),
            doc_version: 1,
            tags: vec!["版本".into()],
            due_at: Some(50),
            pinned: true,
            archived: false,
            reminded: false,
            thumb: None,
            attachments: Vec::new(),
            summary: String::new(),
            text: String::new(),
            created_at: now,
            updated_at: now,
        };
        store.upsert(note).unwrap()
    };
    let saved = store
        .save(NoteSave {
            id: "n_test".into(),
            title: "提测清单".into(),
            doc: sample_doc("登录和支付"),
            doc_version: 1,
            tags: vec!["#版本".into(), "版本".into()],
            due_at: Some(50),
            pinned: true,
            archived: false,
            thumb: None,
            attachments: Vec::new(),
            text: String::new(),
            summary: String::new(),
            base_updated_at: created.updated_at,
        })
        .unwrap();
    assert_eq!(saved.tags, vec!["版本".to_string()]);
    assert_eq!(saved.summary, "登录和支付");
    let err = store
        .save(NoteSave {
            id: "n_test".into(),
            title: "过期".into(),
            doc: empty_doc(),
            doc_version: 1,
            tags: Vec::new(),
            due_at: None,
            pinned: false,
            archived: false,
            thumb: None,
            attachments: Vec::new(),
            text: String::new(),
            summary: String::new(),
            base_updated_at: created.updated_at,
        })
        .unwrap_err();
    assert_eq!(err, "这篇在别处被改过");
    let _ = fs::remove_dir_all(store.data_dir());
}

#[test]
fn corrupt_index_rebuilds_from_notes() {
    let mut store = temp_store();
    let now = now_ms();
    store
        .upsert(Note {
            id: "n_keep".into(),
            title: "还在".into(),
            doc: sample_doc("正文还在"),
            doc_version: 1,
            tags: Vec::new(),
            due_at: None,
            pinned: false,
            archived: false,
            reminded: false,
            thumb: None,
            attachments: Vec::new(),
            summary: "正文还在".into(),
            text: "正文还在".into(),
            created_at: now,
            updated_at: now,
        })
        .unwrap();
    fs::write(&store.index_path, "{").unwrap();
    store.load_index().unwrap();
    assert_eq!(store.index.notes.len(), 1);
    assert_eq!(store.index.notes[0].title, "还在");
    let _ = fs::remove_dir_all(store.data_dir());
}

#[test]
fn reminder_fires_once() {
    let notes = vec![NoteIndex {
        id: "n_due".into(),
        title: "提测清单".into(),
        summary: "回归登录".into(),
        text: String::new(),
        tags: Vec::new(),
        due_at: Some(10),
        pinned: false,
        archived: false,
        reminded: false,
        thumb: None,
        attachments: Vec::new(),
        created_at: 1,
        updated_at: 1,
    }];
    let actions = actions_for(&notes, 20);
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].title, "备忘录 · 提测清单");
    let mut quiet = notes.clone();
    quiet[0].reminded = true;
    assert!(actions_for(&quiet, 20).is_empty());
    let mut later = notes;
    later[0].due_at = Some(30);
    assert!(actions_for(&later, 20).is_empty());
}

#[test]
fn export_markdown_includes_body() {
    let note = Note {
        id: "n_export".into(),
        title: "周报".into(),
        doc: sample_doc("三段式"),
        doc_version: 1,
        tags: vec!["会议".into()],
        due_at: None,
        pinned: false,
        archived: false,
        reminded: false,
        thumb: None,
        attachments: Vec::new(),
        summary: String::new(),
        text: String::new(),
        created_at: 1,
        updated_at: 1,
    };
    let markdown = render_markdown(&note);
    assert!(markdown.contains("# 周报"));
    assert!(markdown.contains("#会议"));
    assert!(markdown.contains("三段式"));
}

#[test]
fn gc_drops_old_orphans_only() {
    let mut store = temp_store();
    let hash = "a".repeat(64);
    let orphan = store.attachments_dir.join(format!("{hash}.png"));
    fs::write(&orphan, b"png").unwrap();
    {
        let file = File::options().write(true).open(&orphan).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(10 * 24 * 3600))
            .unwrap();
    }
    let fresh = store
        .attachments_dir
        .join(format!("{}.png", "b".repeat(64)));
    fs::write(&fresh, b"png").unwrap();
    let kept = "c".repeat(64);
    fs::write(store.attachments_dir.join(format!("{kept}.png")), b"png").unwrap();
    store.index.notes.push(NoteIndex {
        id: "n_img".into(),
        title: "有图".into(),
        summary: String::new(),
        text: String::new(),
        tags: Vec::new(),
        due_at: None,
        pinned: false,
        archived: false,
        reminded: false,
        thumb: Some(kept.clone()),
        attachments: vec![kept],
        created_at: 1,
        updated_at: 1,
    });
    let removed = store.gc_with_grace(Duration::from_secs(1)).unwrap();
    assert!(removed >= 1);
    assert!(!orphan.exists());
    assert!(fresh.exists());
    assert!(
        store
            .attachments_dir
            .join(format!("{}.png", "c".repeat(64)))
            .exists()
    );
    let _ = fs::remove_dir_all(store.data_dir());
}
