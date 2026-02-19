use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about = "HorizOS Package Downloader", long_about = None)]
struct Args {
    /// URL of the binary to download
    #[arg(short, long)]
    url: String,

    /// Name of the binary to save as (in /bin)
    #[arg(short, long)]
    name: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let target_path = format!("/bin/{}", args.name);

    println!("[報告] ダウンロード開始: {}", args.url);

    // rustls-tls を使用した HTTPS リクエスト
    let mut response = reqwest::blocking::get(&args.url)?;

    if !response.status().is_success() {
        return Err(format!("[エラー] ダウンロード失敗: {}", response.status()).into());
    }

    let mut out = fs::File::create(&target_path)?;
    io::copy(&mut response, &mut out)?;

    // 実行権限の付与 (chmod +x)
    let mut perms = out.metadata()?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&target_path, perms)?;

    println!("[報告] インストール完了: {}", target_path);
    Ok(())
}
