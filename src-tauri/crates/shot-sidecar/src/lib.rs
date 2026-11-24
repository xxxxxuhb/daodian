mod host;
mod log;
mod serve;
mod ui;

pub use log::init;
pub use serve::{send_text, serve, ServeOptions};

pub fn run() -> i32 {
    unsafe {
        windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    init();
    let parent = parent_pid();
    let Some(parent) = parent else {
        log::warn("缺少 --parent，截图进程不会脱离主进程单独留下");
        return 2;
    };
    let noop: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
    match serve(ServeOptions {
        parent,
        pipe: shot_core::PIPE_NAME.to_string(),
        mutex: shot_core::MUTEX_NAME.to_string(),
        on_capture: noop.clone(),
        on_dismiss: noop,
        on_config: std::sync::Arc::new(|_| {}),
    }) {
        Ok(()) => 0,
        Err(err) => {
            log::warn(&err);
            1
        }
    }
}

fn parent_pid() -> Option<u32> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--parent" {
            return args.next().and_then(|value| value.parse().ok());
        }
    }
    None
}

#[cfg(all(test, windows))]
mod tests {
    use std::time::Duration;

    use shot_core::{read_frame, write_frame, Decoder, Message, ShotConfig, PROTOCOL_VERSION};

    use super::*;

    #[test]
    fn hello_reload_ping_and_shutdown() {
        let pid = std::process::id();
        let pipe = format!(r"\\.\pipe\daodian-shot-test-{pid}");
        let mutex = format!(r"Local\daodian-shot-test-{pid}");
        let options = ServeOptions {
            parent: pid,
            pipe: pipe.clone(),
            mutex,
            on_capture: std::sync::Arc::new(|| {}),
            on_dismiss: std::sync::Arc::new(|| {}),
            on_config: std::sync::Arc::new(|_| {}),
        };
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let server = std::thread::Builder::new()
            .name("shot-test".into())
            .spawn(move || {
                let result = serve(options);
                let _ = done_tx.send(result);
            })
            .expect("测试服务线程");
        let client = host::connect_client(&pipe);
        if let Err(err) = &client {
            let early = done_rx.recv_timeout(Duration::from_millis(200)).ok();
            panic!("连上测试管道: {err}; 服务结果: {early:?}");
        }
        let mut client = client.expect("连上测试管道");
        let mut decoder = Decoder::default();
        write_frame(
            &mut client,
            &Message::Hello {
                id: 1,
                version: PROTOCOL_VERSION,
            },
        )
        .unwrap();
        match read_frame(&mut client, &mut decoder).unwrap() {
            Message::HelloOk { id, version } => {
                assert_eq!(id, 1);
                assert_eq!(version, PROTOCOL_VERSION);
            }
            other => panic!("握手应答不对: {other:?}"),
        }
        write_frame(
            &mut client,
            &Message::ReloadConfig {
                id: 2,
                config: ShotConfig {
                    enabled: false,
                    shortcut: "Ctrl+1".into(),
                    ..ShotConfig::default()
                },
            },
        )
        .unwrap();
        match read_frame(&mut client, &mut decoder).unwrap() {
            Message::Ready { id, hotkey, .. } => {
                assert_eq!(id, 2);
                assert!(!hotkey);
            }
            other => panic!("配置应答不对: {other:?}"),
        }
        write_frame(&mut client, &Message::Ping { id: 3 }).unwrap();
        assert!(matches!(
            read_frame(&mut client, &mut decoder).unwrap(),
            Message::Pong { id: 3 }
        ));
        write_frame(&mut client, &Message::Shutdown { id: 4 }).unwrap();
        assert!(matches!(
            read_frame(&mut client, &mut decoder).unwrap(),
            Message::Exit { .. }
        ));
        drop(client);
        let finished = done_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("服务没有退出");
        assert!(finished.is_ok(), "{finished:?}");
        let _ = server.join();
    }
}
