//! SSH Tunnel（local port forward）。
//!
//! 連線前若啟用 SSH，開一條 `direct-tcpip` 轉發：在 `127.0.0.1:<OS 分配埠>` 監聽，
//! 每條進站連線都透過 SSH session 轉到原始 DB host:port。driver 連到本地埠即可。
//! `TunnelGuard` 持有關閉旗標與背景任務；drop 前須 `shutdown().await` 收掉。
//!
//! 安全備註：此版本的 `check_server_key` 一律接受（dev 工具；host key TOFU 為後續工作）。

use std::net::SocketAddr;
use std::sync::Arc;

use russh::client::{self, Msg};
use russh::keys::{load_secret_key, PrivateKeyWithHashAlg, PublicKey};
use russh::Channel;
use tokio::io::copy_bidirectional;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::db::{ConnectionConfig, DbKind, SshAuthMethod};
use crate::error::{AppError, AppResult};

/// 不驗 host key 的 client handler（dev 工具）。
struct TunnelHandler;

impl client::Handler for TunnelHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// 一條存活中的 tunnel。本地監聽位址 + 背景任務 + 關閉旗標。
pub struct TunnelGuard {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl TunnelGuard {
    /// 本地轉發埠（driver 改連此埠）。
    pub fn local_port(&self) -> u16 {
        self.local_addr.port()
    }

    /// 通知背景任務結束並等待收尾。
    pub async fn shutdown(self) {
        let _ = self.shutdown.send(true);
        let _ = self.task.await;
    }
}

/// 依連線設定開一條 SSH tunnel，回傳 guard。撥號目標為「原始」DB host:port。
pub async fn open_tunnel(cfg: &ConnectionConfig) -> AppResult<TunnelGuard> {
    if matches!(cfg.kind, DbKind::Sqlite) {
        return Err(AppError::Ssh("SQLite 不支援 SSH Tunnel".into()));
    }
    if cfg.ssh_host.trim().is_empty() {
        return Err(AppError::Ssh("未填寫 SSH 主機".into()));
    }
    let ssh_port = if cfg.ssh_port == 0 { 22 } else { cfg.ssh_port };
    let remote_host = cfg.host.clone();
    let remote_port = cfg.port as u32;

    // 1. 連到 SSH bastion。
    let config = Arc::new(client::Config::default());
    let mut session = client::connect(config, (cfg.ssh_host.as_str(), ssh_port), TunnelHandler)
        .await
        .map_err(|e| AppError::Ssh(format!("SSH 連線失敗：{e}")))?;

    // 2. 認證（密碼或私鑰）。
    let auth = match cfg.ssh_auth_method {
        SshAuthMethod::Password => session
            .authenticate_password(cfg.ssh_username.clone(), cfg.ssh_password.clone())
            .await
            .map_err(|e| AppError::Ssh(format!("SSH 認證失敗：{e}")))?,
        SshAuthMethod::Key => {
            let passphrase = if cfg.ssh_passphrase.is_empty() {
                None
            } else {
                Some(cfg.ssh_passphrase.as_str())
            };
            let key = load_secret_key(&cfg.ssh_private_key_path, passphrase)
                .map_err(|e| AppError::Ssh(format!("讀取 SSH 私鑰失敗：{e}")))?;
            let key = PrivateKeyWithHashAlg::new(Arc::new(key), None);
            session
                .authenticate_publickey(cfg.ssh_username.clone(), key)
                .await
                .map_err(|e| AppError::Ssh(format!("SSH 認證失敗：{e}")))?
        }
    };
    if !auth.success() {
        return Err(AppError::Ssh("SSH 認證被拒（帳號 / 密碼 / 金鑰不正確）".into()));
    }

    // 3. 本地監聽（OS 分配空埠，避免手動掃描競態）。
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| AppError::Ssh(format!("本地監聽失敗：{e}")))?;
    let local_addr = listener
        .local_addr()
        .map_err(|e| AppError::Ssh(format!("取得本地埠失敗：{e}")))?;

    // 4. 背景 accept loop：每條進站連線開一條 direct-tcpip 並雙向轉送。
    let (tx, mut rx) = watch::channel(false);
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = rx.changed() => {
                    if *rx.borrow() {
                        break;
                    }
                }
                accepted = listener.accept() => {
                    let (mut socket, peer) = match accepted {
                        Ok(v) => v,
                        Err(e) => {
                            eprintln!("[ssh] accept 失敗：{e}");
                            break;
                        }
                    };
                    let channel: Channel<Msg> = match session
                        .channel_open_direct_tcpip(
                            remote_host.clone(),
                            remote_port,
                            "127.0.0.1".to_string(),
                            peer.port() as u32,
                        )
                        .await
                    {
                        Ok(c) => c,
                        Err(e) => {
                            eprintln!("[ssh] 開啟轉發通道失敗：{e}");
                            continue;
                        }
                    };
                    tokio::spawn(async move {
                        let mut stream = channel.into_stream();
                        let _ = copy_bidirectional(&mut socket, &mut stream).await;
                    });
                }
            }
        }
        // 跳出迴圈後 session（Handle）隨任務結束 drop，russh 會關閉連線。
        drop(session);
    });

    Ok(TunnelGuard {
        local_addr,
        shutdown: tx,
        task,
    })
}
