use std::process::Command;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use std::ffi::CString;
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use libc::{mount, MS_NOSUID, MS_NODEV, MS_NOEXEC, waitpid, WNOHANG, SIGCHLD, signal, SIG_DFL, SIG_IGN};

use horiz_auth;

/// ログレベルの定義
enum LogLevel {
    Info,
    Warn,
    Error,
    Audit,
}

impl LogLevel {
    fn as_str(&self) -> &str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Warn => "WARN",
            LogLevel::Error => "ERROR",
            LogLevel::Audit => "AUDIT",
        }
    }
}

/// タイムスタンプを取得 (Zero-Dependency)
fn get_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 構造化ログを出力
fn log_message(level: LogLevel, message: &str) {
    let ts = get_timestamp();
    let log_entry = format!("[{}] [{}] {}\n", ts, level.as_str(), message);
    
    // 標準出力への報告
    match level {
        LogLevel::Error => eprintln!("{}", log_entry.trim()),
        _ => println!("{}", log_entry.trim()),
    }

    // ログファイルへの永続化
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/var/log/system.log") {
        let _ = f.write_all(log_entry.as_bytes());
    }
    
    if let LogLevel::Audit = level {
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/var/log/audit.log") {
            let _ = f.write_all(log_entry.as_bytes());
        }
    }
}

fn mount_fs(source: &str, target: &str, fstype: &str, flags: u64) {
    let c_source = CString::new(source).unwrap();
    let c_target = CString::new(target).unwrap();
    let c_fstype = CString::new(fstype).unwrap();

    unsafe {
        if mount(
            c_source.as_ptr(),
            c_target.as_ptr(),
            c_fstype.as_ptr(),
            flags,
            std::ptr::null(),
        ) == 0
        {
            log_message(LogLevel::Info, &format!("{} をマウント完了。", target));
        } else {
            log_message(LogLevel::Warn, &format!("{} のマウントに失敗しました。", target));
        }
    }
}

fn setup_network() {
    log_message(LogLevel::Info, "ネットワークインターフェースを初期化中...");
    
    let mut success = false;
    for i in 1..=3 {
        log_message(LogLevel::Info, &format!("ループバックインターフェース (lo) の有効化を試行中 (回数: {}/3)...", i));
        let status = Command::new("ip").args(&["link", "set", "lo", "up"]).status();
        match status {
            Ok(s) if s.success() => {
                log_message(LogLevel::Info, "ループバックインターフェース (lo) を有効化。");
                success = true;
                break;
            }
            Ok(s) => log_message(LogLevel::Warn, &format!("ip コマンドがエラーを返しました (status: {})。", s)),
            Err(e) => log_message(LogLevel::Error, &format!("ip コマンドの実行に失敗: {}", e)),
        }
        thread::sleep(Duration::from_secs(1));
    }

    if !success {
        log_message(LogLevel::Error, "ネットワーク初期化に致命的な失敗が発生しました。一部の機能が制限される可能性があります。");
    }
}

fn reap_zombies() {
    unsafe {
        loop {
            let mut status = 0;
            let pid = waitpid(-1, &mut status, WNOHANG);
            if pid <= 0 {
                break;
            }
            log_message(LogLevel::Info, &format!("ゾンビプロセスを回収: PID {}", pid));
        }
    }
}

fn login_prompt() -> String {
    loop {
        println!("\n--- HorizOS Login ---");
        print!("username: ");
        io::stdout().flush().unwrap();
        let mut username = String::new();
        io::stdin().read_line(&mut username).unwrap();
        let username = username.trim().to_string();

        if username.is_empty() { continue; }

        print!("password: ");
        io::stdout().flush().unwrap();
        let mut password = String::new();
        io::stdin().read_line(&mut password).unwrap();
        let password = password.trim();

        match horiz_auth::verify_login(&username, password) {
            Ok(true) => {
                log_message(LogLevel::Info, &format!("認証成功。ユーザー: {}", username));
                log_message(LogLevel::Audit, &format!("Successful login for user: {}", username));
                return username;
            }
            Ok(false) => {
                log_message(LogLevel::Warn, &format!("ログイン失敗。ユーザー: {}", username));
                log_message(LogLevel::Audit, &format!("Failed login attempt for user: {}", username));
            }
            Err(e) => {
                log_message(LogLevel::Error, &format!("認証システムエラー: {}", e));
            }
        }
    }
}

fn run_session(user: &str) {
    env::set_var("USER", user);
    log_message(LogLevel::Info, &format!("ユーザーステータスを開始: {}", user));

    loop {
        reap_zombies();
        match Command::new("/bin/sh").spawn() {
            Ok(mut child) => {
                log_message(LogLevel::Info, &format!("シェルプロセスを起動 (PID: {})", child.id()));
                match child.wait() {
                    Ok(status) => {
                        log_message(LogLevel::Warn, &format!("セッションが終了しました (status: {})。再起動します。", status));
                        log_message(LogLevel::Audit, &format!("Session ended for user: {} (status: {})", user, status));
                    }
                    Err(e) => {
                        log_message(LogLevel::Error, &format!("Wait失敗: {}", e));
                        break;
                    }
                }
            }
            Err(e) => {
                log_message(LogLevel::Error, &format!("シェルの起動に失敗: {}. 1秒後に再試行します。", e));
                thread::sleep(Duration::from_secs(1));
            }
        }
    }
}

fn main() {
    println!("--- HorizOS Core Initializing (Enhanced) ---");

    // シグナルハンドリングの初期化 (SIGCHLDをデフォルトに)
    unsafe {
        signal(SIGCHLD, SIG_DFL);
    }

    // 1. 仮想ファイルシステムのマウント
    mount_fs("proc", "/proc", "proc", 0);
    mount_fs("sysfs", "/sys", "sysfs", 0);
    mount_fs("devtmpfs", "/dev", "devtmpfs", 0);
    mount_fs("tmpfs", "/tmp", "tmpfs", MS_NOSUID | MS_NODEV | MS_NOEXEC);

    // 必須ディレクトリの作成
    let _ = fs::create_dir_all("/var/log");

    // 2. ネットワークセットアップ
    setup_network();

    log_message(LogLevel::Info, "システム初期化完了。");

    loop {
        let user = login_prompt();
        run_session(&user);
    }
}

