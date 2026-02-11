fn main() {
    ensure_default_icons_exist();
    tauri_build::build()
}

fn ensure_default_icons_exist() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let icons_dir = std::path::Path::new(&manifest_dir).join("icons");
    let _ = std::fs::create_dir_all(&icons_dir);

    // A tiny 1x1 PNG placeholder so `tauri::generate_context!()` can compile.
    // Real app icons are generated later via `tauri icon`.
    let png_path = icons_dir.join("icon.png");
    if !png_path.exists() {
        let png_bytes: &[u8] = &[
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 96, 96, 96,
            248, 15, 0, 1, 4, 1, 0, 95, 229, 195, 75, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
        ];
        let _ = std::fs::write(&png_path, png_bytes);
    }

    // A minimal 1x1 ICO placeholder for Windows builds.
    // ICO header (6 bytes) + 1 directory entry (16 bytes) + 1x1 32-bit BMP data.
    let ico_path = icons_dir.join("icon.ico");
    if !ico_path.exists() {
        let mut ico: Vec<u8> = Vec::new();
        // ICO header: reserved=0, type=1 (icon), count=1
        ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        // Directory entry: 1x1, 0 colors, 0 reserved, 1 plane, 32 bpp
        // data size = 40 (BITMAPINFOHEADER) + 4 (1px BGRA) + 4 (1px AND mask)
        let data_size: u32 = 48;
        let data_offset: u32 = 22; // 6 header + 16 dir entry
        ico.extend_from_slice(&[1, 1, 0, 0, 1, 0, 32, 0]);
        ico.extend_from_slice(&data_size.to_le_bytes());
        ico.extend_from_slice(&data_offset.to_le_bytes());
        // BITMAPINFOHEADER (40 bytes)
        ico.extend_from_slice(&40u32.to_le_bytes()); // biSize
        ico.extend_from_slice(&1i32.to_le_bytes()); // biWidth
        ico.extend_from_slice(&2i32.to_le_bytes()); // biHeight (doubled for ICO)
        ico.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        ico.extend_from_slice(&32u16.to_le_bytes()); // biBitCount
        ico.extend_from_slice(&[0; 24]); // rest zeroed
                                         // 1 pixel BGRA (transparent)
        ico.extend_from_slice(&[0, 0, 0, 0]);
        // AND mask (1 byte row padded to 4 bytes)
        ico.extend_from_slice(&[0xFF, 0, 0, 0]);
        let _ = std::fs::write(&ico_path, ico);
    }
}
