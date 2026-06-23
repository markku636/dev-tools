# 品牌素材（MAGIDB Connect）

原始高解析插畫與「去背」流程的存檔，方便日後重新匯出。

| 檔案 | 用途 |
| --- | --- |
| `logo.png` | 原始標誌（海豹吉祥物 + 字標），含近白底 |
| `connect.png` | 原始「連線資料庫」圖示，含近白底 |
| `remove-bg.py` | 去背腳本：邊緣 flood-fill 移除近白底、羽化抗鋸齒邊、自動裁切、縮放 |

## 產生的素材（已被 App 使用，請勿手改）

`remove-bg.py` 會輸出到 [`../src/assets/`](../src/assets/)：

- `logo-mark.png` — 透明背景標誌（開場動畫 `SplashScreen` 使用）
- `connect-icon.png` — 透明背景圖示（工具列「連線」按鈕使用）

## 重新匯出

```bash
pip install Pillow numpy scipy
python brand/remove-bg.py
```

去背原理：把與邊框相連的近白像素以 flood-fill 標為背景並設為透明；被插畫包圍的白色像素（如海豹白肚）因不與邊框相連而保留。邊界數像素帶內依「彩度」羽化 alpha，避免殘留白色光暈。
