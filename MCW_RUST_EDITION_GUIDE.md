# Bản Tóm Tắt Khởi Tạo — MCW Launcher 2.0 (Rust Edition)

> Tài liệu này dùng làm **bản đồ dẫn đường (Kickoff Blueprint)** cho repo Rust mới ở thư mục riêng. Bạn có thể copy trực tiếp file này sang repo mới (ví dụ đặt tên là `BLUEPRINT.md` hoặc `docs/ARCHITECTURE_PLAN.md`) để bản thân hoặc AI Agent ở thư mục mới nắm rõ **cần làm gì, làm theo thứ tự nào, và lấy logic/code mẫu/dữ liệu từ đâu** trong bản Python `1.8.x`.

---

## 1. Triết lý & Định danh Dự án

- **Định danh thế hệ**: **MCW Launcher 2.0 (Rust Edition)** — viết lại toàn bộ cả **Core (Rust)** và **GUI mới** trên một repository độc lập.
- **Lộ trình đánh số phiên bản**: Bắt đầu bài bản từ **`v0.1.0-alpha.1` → `v0.2.x` → `v0.3.x` → ...** theo đúng kỷ luật phát triển của nhánh Python (mỗi phiên bản giải quyết gọn 1–2 phân hệ, có unit test + chạy kiểm chứng thật trước khi sang bước tiếp theo).
- **Nguồn đặc tả sống (Reference Spec)**: Repo Python hiện tại tại `d:\WORKBASE\mcw-launcher` (nhánh `1.8.7`) đóng vai trò là **tài liệu đặc tả nghiệp vụ và bộ kiểm thử chuẩn** (chứa toàn bộ edge-case đã được kiểm chứng thực tế).
- **Hạ tầng phân phối dùng chung**: `https://mcw-download.pages.dev` (repo `mahiru7229/mcw-download`).

---

## 2. Cấu trúc Cargo Workspace Đề Xuất Cho Repo Mới

```text
mcw-launcher-rs/
├── Cargo.toml                  # Workspace root (resolver = "2")
├── BLUEPRINT.md                # Tài liệu này
├── assets/                     # Copy từ d:\WORKBASE\mcw-launcher\assets
├── lang/                       # Copy tham chiếu từ d:\WORKBASE\mcw-launcher\lang
├── tools/
│   └── mcw_lan_agent/          # Copy nguyên từ d:\WORKBASE\mcw-launcher\tools\mcw_lan_agent (Java thuần)
├── crates/
│   ├── mcw-models/             # Kiểu dữ liệu cốt lõi (Instance, Settings, Account, Version, Progress)
│   ├── mcw-storage/            # Quản lý đường dẫn, đọc/ghi atomic, Backup/Restore, Migration từ 1.8.x
│   ├── mcw-net/                # Download Engine (tokio + reqwest), SHA verify, resume, rate-limit, mirror
│   ├── mcw-auth/               # Offline auth + Microsoft OAuth (PKCE + Device Code) + OS Keyring/DPAPI
│   ├── mcw-runtime/            # Java discovery/provisioning, Process Supervisor (windows-rs), Live Logs, Crash Analyzer
│   ├── mcw-minecraft/          # Mojang Manifest, Client/Libs/Assets, 4 Mod Loaders, OptiFine, Launch Builder
│   ├── mcw-mods/               # Quét .jar song song (rayon + zip), Jar-in-Jar, đọc bytecode .class chọn Java
│   ├── mcw-providers/          # Modrinth v2, CurseForge API v1, FTB, ATLauncher + SQLite cache
│   └── mcw-updater/            # Kiểm tra & áp dụng cập nhật (Cloudflare CDN + GitHub failover)
└── apps/
    ├── cli/                    # CLI nhỏ (mcw-cli) dùng để test Core ngay từ v0.1.0-alpha.1
    └── desktop/                # Giao diện GUI mới (Tauri v2 + Web UI hoặc Slint/Iced) — bắt đầu từ v0.3
```

### Các Crate Rust Chuẩn Khuyến Nghị
- **Async & Mạng**: `tokio`, `reqwest` (rustls), `futures`, `governor` (rate-limit)
- **Dữ liệu & Lỗi**: `serde`, `serde_json`, `thiserror`, `anyhow` (ở tầng app/CLI), `uuid`, `chrono`
- **Hiệu năng & File**: `rayon`, `zip`, `sha1`, `sha2`, `walkdir`, `tempfile`
- **Cơ sở dữ liệu & Bảo mật**: `rusqlite` (bundled), `keyring` / `windows` (DPAPI + Job Object + Win32 API)
- **Log & CLI**: `tracing`, `tracing-subscriber`, `clap`

---

## 3. Bản Đồ Tham Chiếu: Lấy Logic, Schema & Code Mẫu Ở Đâu?

Khi viết từng crate ở repo mới, **không cần đoán lại nghiệp vụ Minecraft** — mở đúng file tham chiếu tương ứng trong `d:\WORKBASE\mcw-launcher`:

| Crate mới (Rust) | Thư mục / File tham chiếu trong `d:\WORKBASE\mcw-launcher` | Nội dung & Edge-case cần lấy làm chuẩn |
| :--- | :--- | :--- |
| **`mcw-models`** & **`mcw-storage`** | - [`src/config.py`](file:///d:/WORKBASE/mcw-launcher/src/config.py)<br>- [`src/core/fs/paths.py`](file:///d:/WORKBASE/mcw-launcher/src/core/fs/paths.py)<br>- [`src/core/instance/instance_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/instance/instance_manager.py)<br>- [`src/core/instance/settings_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/instance/settings_manager.py)<br>- [`src/core/backup/instance_backup_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/backup/instance_backup_manager.py) | - Cấu trúc thư mục `instances/`, `cache/`, `runtimes/`, `accounts/`, `logs/`.<br>- Schema `instance.json`, `settings.json` (để tương thích/migrate từ `1.8.x`).<br>- Quy tắc đặt tên instance hợp lệ trên Windows, cơ chế lock (`instance_run_lock.py`) và rollback backup an toàn. |
| **`mcw-net`** | - [`src/core/network/download_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/network/download_manager.py)<br>- [`src/core/network/artifact_download_service.py`](file:///d:/WORKBASE/mcw-launcher/src/core/network/artifact_download_service.py)<br>- [`src/core/network/download_journal.py`](file:///d:/WORKBASE/mcw-launcher/src/core/network/download_journal.py)<br>- [`src/core/repair/verification_cache.py`](file:///d:/WORKBASE/mcw-launcher/src/core/repair/verification_cache.py) | - Chuẩn `User-Agent`, retry backoff, resume bằng HTTP `Range`, xử lý `416`/`206`.<br>- Xác thực `size` + `sha1`/`sha256`/`sha512`, tải mirror failover.<br>- Kiểm tra nhanh `(path, mtime, size)` qua `VerificationCache`. |
| **`mcw-auth`** | - [`src/core/auth/offline_auth.py`](file:///d:/WORKBASE/mcw-launcher/src/core/auth/offline_auth.py)<br>- [`src/core/auth/microsoft/microsoft_oauth.py`](file:///d:/WORKBASE/mcw-launcher/src/core/auth/microsoft/microsoft_oauth.py)<br>- [`src/core/auth/microsoft/oauth_callback_server.py`](file:///d:/WORKBASE/mcw-launcher/src/core/auth/microsoft/oauth_callback_server.py)<br>- [`src/core/account/database/account_database.py`](file:///d:/WORKBASE/mcw-launcher/src/core/account/database/account_database.py) | - `MICROSOFT_CLIENT_ID` (`cd379605-ee06-466a-a588-7a1f7c23b48a`) và chuỗi luồng: Microsoft OAuth (PKCE / Device Code) → Xbox Live (XBL) → XSTS → Minecraft Services Token → Profile & Skin.<br>- Tạo Offline UUID chuẩn (`OfflinePlayer:<name>` MD5 v3).<br>- Lưu trữ bảo mật (không lưu Minecraft access token dài hạn dạng thuần). |
| **`mcw-runtime`** | - [`src/core/java/java_major_policy.py`](file:///d:/WORKBASE/mcw-launcher/src/core/java/java_major_policy.py)<br>- [`src/core/java/java_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/java/java_manager.py)<br>- [`src/core/java/java_resolver.py`](file:///d:/WORKBASE/mcw-launcher/src/core/java/java_resolver.py)<br>- [`src/core/java/java_runtime.py`](file:///d:/WORKBASE/mcw-launcher/src/core/java/java_runtime.py)<br>- [`src/core/runtime/process_supervisor.py`](file:///d:/WORKBASE/mcw-launcher/src/core/runtime/process_supervisor.py)<br>- [`src/core/runtime/game_runtime_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/runtime/game_runtime_manager.py) | - Chính sách chọn Java `8 / 17 / 21 / 25` (Adoptium/Temurin).<br>- Tự động nâng Java dựa trên bytecode `.class` của mod (`class_major - 44`) và khi crash `UnsupportedClassVersionError`.<br>- Giám sát tiến trình, phân biệt dừng chủ động vs crash, đo thời gian chơi, đọc crash report. |
| **`mcw-minecraft`** | - [`src/core/minecraft/version_manifest_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/version_manifest_manager.py)<br>- [`src/core/minecraft/library_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/library_manager.py)<br>- [`src/core/minecraft/library_rule_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/library_rule_manager.py)<br>- [`src/core/minecraft/asset_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/asset_manager.py)<br>- [`src/core/minecraft/argument_builder.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/argument_builder.py)<br>- [`src/core/minecraft/classpath_builder.py`](file:///d:/WORKBASE/mcw-launcher/src/core/minecraft/classpath_builder.py)<br>- [`src/core/modloader/`](file:///d:/WORKBASE/mcw-launcher/src/core/modloader) | - Parsing `version_manifest_v2.json`, rules OS/arch/features, giải nén natives, xây dựng classpath & JVM/Game arguments (kể cả Quick Play, Windows command-line length limit `@argfile`).<br>- Cách cài đặt & hợp nhất profile cho **Fabric, Quilt, Forge (1.12.2 / 1.16.5 / 1.17+), NeoForge** và **OptiFine**. |
| **`mcw-mods`** | - [`src/core/mod/mod_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/mod/mod_manager.py)<br>- [`src/core/mod/mod_capability_index.py`](file:///d:/WORKBASE/mcw-launcher/src/core/mod/mod_capability_index.py)<br>- [`src/core/mod/modpack_dependency_resolver.py`](file:///d:/WORKBASE/mcw-launcher/src/core/mod/modpack_dependency_resolver.py) | - Đọc `fabric.mod.json`, `quilt.mod.json`, `META-INF/mods.toml`, `META-INF/neoforge.mods.toml`, `mcmod.info` và Jar-in-Jar (`META-INF/jars/*.jar`, `META-INF/jarjar/*.jar`).<br>- Cách xếp hạng & kiểm tra tương thích loader (ví dụ Fabric-on-Quilt). |
| **`mcw-providers`** | - [`src/core/modrinth/`](file:///d:/WORKBASE/mcw-launcher/src/core/modrinth)<br>- [`src/core/curseforge/curseforge_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/curseforge/curseforge_client.py)<br>- [`src/core/ftb/ftb_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/ftb/ftb_client.py)<br>- [`src/core/atlauncher/atlauncher_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/atlauncher/atlauncher_client.py) | - Endpoints, query params, sort field, loader mapping, phân trang, tải batch 50 file, xử lý file bị tắt third-party distribution (manual download fallback) và lọc `gameVersion` cho mod >50 file trên CurseForge API v1. |
| **`mcw-updater`** | - [`src/core/update/update_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/update/update_manager.py)<br>- [`src/core/update/cloudflare_release_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/update/cloudflare_release_client.py)<br>- [`src/core/update/github_release_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/update/github_release_client.py) | - Quy tắc kiểm tra phiên bản (`LauncherVersion`), Cloudflare CDN (`mcw-download.pages.dev`) + GitHub Releases failover trên mọi kênh (`stable` và `beta`). |

---

## 4. Tài Nguyên Có Thể Tái Sử Dụng Trực Tiếp (Copy Sang Repo Mới)

1. **`tools/mcw_lan_agent/`**: Mã nguồn Java thuần của **MCW LAN Agent** — không phụ thuộc Python, dùng lại 100% cho bản Rust.
2. **`assets/`**: Bộ biểu tượng, logo, icon trạng thái của MCW Launcher.
3. **`lang/en-US.json` & `lang/vi-VN.json`**: Kho từ điển song ngữ Anh – Việt sẵn có (tái sử dụng thuật ngữ chuẩn của MCW).
4. **`LICENSE` & `SECURITY.md`**: Điều khoản bản quyền (kèm quy định fork / định danh dự án / API bên thứ ba) và chính sách bảo mật.
5. **`test/` (304 file test)**: Dùng làm danh sách test-case tham chiếu cho các bộ test `#[test]` / integration test của Rust.

---

## 5. Lộ Trình Build Từng Phiên Bản (Từ `v0.1.0-alpha.1`)

| Phiên bản | Mục tiêu cốt lõi | Tiêu chí nghiệm thu (Done) |
| :--- | :--- | :--- |
| **`v0.1.0-alpha.1`** | Khởi tạo Workspace + `mcw-models` + `mcw-storage` + `mcw-net` + `mcw-auth` + `mcw-runtime` (Java cơ bản) + `mcw-minecraft` (**Vanilla**) + `apps/cli`. | Chạy được `mcw-cli launch --instance test --version 1.21.4 --offline Player` và **vào được màn hình chính Minecraft Vanilla** hoàn chỉnh, có tải song song + verify SHA-1. |
| **`v0.2.0-alpha.1`** | Hoàn thiện **Microsoft OAuth** (PKCE + Device Code), quản lý Instance (CRUD), **Fabric & Quilt**, tự động tải/chọn Java (kèm quét bytecode `.class`), Process Supervisor (Win32 Job Object) & Live Logs. | Đăng nhập tài khoản Microsoft thật, tạo & chơi được Instance **Fabric 1.16.1 + WorldPreview** (tự chọn Java 17) và **Fabric 1.21.x**, dừng/buộc dừng game không báo nhầm crash. |
| **`v0.3.0-alpha.1`** | Dựng **GUI thế hệ mới (`apps/desktop`)** bản đầu tiên gắn trực tiếp lên Core Rust (Danh sách Instance, Tạo Instance, Tài khoản, Cài đặt Instance, Nút Play + Progress stream mượt, Tab Logs). | Người dùng sử dụng hoàn toàn bằng giao diện đồ họa, không cần mở CLI. |
| **`v0.4.0-alpha.1`** | Thêm `mcw-mods` (`rayon` + `zip`) và `mcw-providers` (**Modrinth v2** + **CurseForge API v1** + SQLite cache), trình duyệt Mod/Modpack & Resource/Shader Pack, hộp thoại tải thủ công. | Cài được mod lẻ & modpack lớn (>300 mod) từ Modrinth và CurseForge nhanh, mượt, không khựng UI. |
| **`v0.5.0-beta.1`** | Hỗ trợ đầy đủ **Forge (1.12.2 / 1.16.5 / 1.17+), NeoForge, OptiFine**, **FTB & ATLauncher**, **Backup/Restore**, **Repair**, và **MCW LAN Agent**. | Đạt đầy đủ 100% tính năng của nhánh Python `1.8.7`. |
| **`v0.6.0-rc.1` → `2.0`** | **Migration tự động từ `1.8.x`**, **Auto-Updater** (Cloudflare CDN + GitHub failover + chữ ký số), hoàn thiện đa ngôn ngữ EN/VI, CI/CD đóng gói Windows & Linux. | Sẵn sàng phát hành chính thức **MCW Launcher 2.0 (Rust Edition)**. |

---

## 6. Những Bài Học "Xương Máu" Từ `1.8.x` Phải Thiết Kế Đúng Ngay Từ Đầu

1. **Maven Library `size == 0`**: Cho phép `size >= 0` (chỉ kiểm tra kích thước khi `expected_size > 0`, luôn lấy `sha1` làm chuẩn) vì nhiều server Maven của Fabric/Quilt/Forge không trả `Content-Length`.
2. **Tự động nâng Java từ Bytecode `.class`**: Với Fabric/Quilt, luôn quét 8 byte header (`0xCAFEBABE`, `major`) của các `.class` trong mod `.jar` (`class_major - 44`) để tự nâng Java khi cần; và cho phép người dùng chọn Java cao hơn cho Vanilla/Fabric/Quilt.
3. **Fallback cập nhật trên MỌI kênh**: Khi Cloudflare CDN trả về rỗng hoặc chưa kịp đồng bộ, **cả kênh `stable` lẫn `beta`** đều phải tự động kiểm tra dự phòng qua GitHub Releases API.
4. **OAuth Callback Server**: Luôn **bind và lắng nghe cổng `127.0.0.1:8400` xong xuôi trước** rồi mới mở trình duyệt.
5. **Cache không nghẽn đĩa**: Lưu cache API Provider trong SQLite (WAL mode) hoặc bộ nhớ đệm RAM, tuyệt đối không `fsync` toàn bộ index trên mỗi lần đọc cache hit.
6. **An toàn dữ liệu tuyệt đối**: Mọi thao tác ghi file cấu hình dùng atomic temp-file + rename (có retry ngắn nếu gặp Windows `PermissionError`); thao tác Restore Backup không bao giờ xóa thư mục `rollback` nếu quá trình khôi phục thất bại.
7. **Quản lý tiến trình trên Windows**: Dùng trực tiếp Win32 API (`windows-rs`: Job Object / `QueryFullProcessImageNameW` / `GetProcessTimes`), **không** gọi `powershell.exe Get-CimInstance`, và lưu rõ trạng thái dừng chủ động để không báo nhầm Game Crash.
8. **Không trộn trạng thái lỗi cũ vào thanh tiến trình mới**: Thanh tiến trình chỉ phản ánh trạng thái của đợt chạy hiện tại, không đọc lại lịch sử lỗi cũ khi tác vụ mới vừa hoàn tất 100%.
