"""Edge flood-fill background remover.

Removes the near-white background of MAGIDB art while preserving white
pixels enclosed inside the artwork (e.g. the seal's white body), feathers
the anti-aliased ring so no white halo remains, then auto-crops to content.
"""
import sys
import numpy as np
from PIL import Image
from scipy import ndimage


def remove_bg(src, dst, *, white_hard=22, feather_lo=12, feather_hi=130,
              dilate=3, max_dim=None, pad=0):
    im = Image.open(src).convert("RGBA")
    arr = np.array(im).astype(np.float32)
    rgb = arr[..., :3]
    alpha = arr[..., 3].copy()

    # Distance from pure white: 0 = white, large = saturated colour.
    dist = np.sqrt(((255.0 - rgb) ** 2).sum(axis=2))

    # 1) Hard near-white mask, keep only the component(s) touching the border.
    near_white = dist < white_hard
    labels, _ = ndimage.label(near_white)
    border = np.concatenate([labels[0, :], labels[-1, :], labels[:, 0], labels[:, -1]])
    border_ids = set(np.unique(border)) - {0}
    bg = np.isin(labels, list(border_ids))

    # 2) Feather the anti-aliased ring just outside the cut so no white halo
    #    survives. In a few-px band around bg, alpha scales with "colourfulness".
    band = ndimage.binary_dilation(bg, iterations=dilate) & ~bg
    soft = np.clip((dist - feather_lo) / (feather_hi - feather_lo), 0.0, 1.0)

    out_alpha = alpha.copy()
    out_alpha[bg] = 0.0
    out_alpha[band] = np.minimum(out_alpha[band], soft[band] * 255.0)

    out = arr.copy()
    out[..., 3] = out_alpha
    out_img = Image.fromarray(out.astype(np.uint8), "RGBA")

    # 3) Auto-crop to the opaque bounding box (+ optional padding).
    bbox = out_img.getbbox()
    if bbox:
        l, t, r, b = bbox
        l = max(0, l - pad); t = max(0, t - pad)
        r = min(out_img.width, r + pad); b = min(out_img.height, b + pad)
        out_img = out_img.crop((l, t, r, b))

    # 4) Optional downscale (high-quality) to cap bundle size.
    if max_dim and max(out_img.size) > max_dim:
        s = max_dim / max(out_img.size)
        out_img = out_img.resize(
            (round(out_img.width * s), round(out_img.height * s)), Image.LANCZOS)

    out_img.save(dst)
    pct = 100.0 * (out_alpha < 4).sum() / out_alpha.size
    print(f"{src} -> {dst}  size={out_img.size}  transparent={pct:.1f}%")


if __name__ == "__main__":
    import os
    here = os.path.dirname(os.path.abspath(__file__))   # brand/
    assets = os.path.join(os.path.dirname(here), "src", "assets")
    remove_bg(os.path.join(here, "logo.png"),
              os.path.join(assets, "logo-mark.png"),    max_dim=1000, pad=6)
    remove_bg(os.path.join(here, "connect.png"),
              os.path.join(assets, "connect-icon.png"), max_dim=256,  pad=4)
