# Bản Tóm Tắt Khởi Tạo — MCW Launcher 2.0 (Rust Edition)

> Tài liệu này dùng làm **bản đồ dẫn đường (Kickoff Blueprint)** cho repo Rust mới ở thư mục riêng. Bạn có thể copy trực tiếp file này sang repo mới (ví dụ đặt tên là `BLUEPRINT.md` hoặc `docs/ARCHITECTURE_PLAN.md`) để bản thân hoặc AI Agent ở thư mục mới nắm rõ **cần làm gì, làm theo thứ tự nào, và lấy logic/code mẫu/dữ liệu từ đâu** trong bản Python `1.8.x`.

---

## RULE Bắt Buộc (Core Development Rule)

> [!IMPORTANT]
> **100% Dựa Trên Codebase MCW Launcher Python (`d:\WORKBASE\mcw-launcher`)**
>
> - **Bám sát 100% (hoặc gần giống nhất có thể) kiến trúc, logic nghiệp vụ, schema dữ liệu, luồng xử lý và UI/UX của MCW Launcher Python.**
> - **Không tự ý sáng tạo/tạo code hoặc cơ chế mới** — luôn kiểm tra và **tái sử dụng** tối đa logic, cấu trúc, tài nguyên (`lang/en-US.json`, `lang/vi-VN.json`, `assets/`, `tools/mcw_lan_agent/`, schema `instance.json` / `settings.json`, thuật toán & edge-case) từ codebase Python.
> - **Ngoại lệ duy nhất**: Chỉ được viết mới hoặc điều chỉnh khác đi khi giải pháp của Python **không phù hợp** với Rust/nền tảng native (ví dụ: giới hạn thư viện GUI `egui` so với `PySide6`, Win32 API thay cho gọi PowerShell, hoặc tối ưu hiệu năng/bộ nhớ/an toàn kiểu dữ liệu của Rust). Khi điều chỉnh, vẫn phải giữ nguyên hành vi và kết quả đầu ra tương đương bản Python.
- **Quy ước Build / Cập nhật (Không dùng cơ chế Hotfix riêng)**: **MCW Launcher: Rust Edition** không triển khai hệ thống `hotfix` tách rời như bản Python; dùng **SemVer tag bình thường** (`v0.6.0-beta.1` → `v0.6.0-beta.2`, hoặc `v0.6.0` → `v0.6.1` → `v0.6.2`), trong đó **chỉ số cuối** xác định số build/bản vá tăng dần (khớp hoàn toàn với Cargo và `mcw-updater`).
- **Quy chuẩn UX & Hiệu năng chung**:
  1. **Cửa sổ riêng cho trình duyệt tài nguyên**: Mọi luồng tìm kiếm/duyệt/tải tài nguyên ngoài (Mods, Modpacks, Resource Packs, Shader Packs...) phải mở ở cửa sổ OS riêng biệt (`show_viewport_immediate`); danh sách tài nguyên trong tab luôn có `ScrollArea` tự do cuộn toàn chiều cao.
  2. **Dùng Dropdown (`ComboBox`) cho tùy chọn nhiều lựa chọn**: Không dàn hàng ngang nhiều nút bấm cho các tùy chọn chọn 1 trong nhiều giá trị (trừ khi được yêu cầu riêng).
  3. **Hộp thoại chọn nơi lưu file & Thông báo tự đóng**: Mọi thao tác lưu/xuất file đều mở `SaveFileDialog`; thông báo có nút *Dismiss* tự động đóng sau 8 giây (có cài đặt chỉnh trong *Launcher Settings*).
  4. **Verification Cache cho Assets & Libraries**: Không tải lại hoặc băm lại file không đổi giữa các lần chạy.

---

## 1. Triết lý & Định danh Dự án

- **Định danh thế hệ**: **MCW Launcher 2.0 (Rust Edition)** — viết lại toàn bộ cả **Core (Rust)** và **GUI mới** trên một repository độc lập.
- **Lộ trình đánh số phiên bản**: Bắt đầu bài bản từ **`v0.1.0-alpha.1` → `v0.2.x` → `v0.3.x` → ...** theo đúng kỷ luật phát triển của nhánh Python (mỗi phiên bản giải quyết gọn 1–2 phân hệ, có unit test + chạy kiểm chứng thật trước khi sang bước tiếp theo).
- **Nguồn đặc tả sống (Reference Spec)**: Repo Python hiện tại tại `d:\WORKBASE\mcw-launcher` (nhánh `1.8.7`) đóng vai trò là **tài liệu đặc tả nghiệp vụ và bộ kiểm thử chuẩn** (chứa toàn bộ edge-case đã được kiểm chứng thực tế).
- **Kênh phân phối tách biệt hoàn toàn khỏi bản Python**: Toàn bộ nhánh cập nhật của MCW Launcher Python (bao gồm Cloudflare CDN `mcw-download.pages.dev`) đã được gỡ sạch; `mcw-updater` chỉ nhắm repo Rust Edition (`mahiru7229/mcw-launcher-rust`) và đang tắt mặc định (`UPDATES_ENABLED = false`).

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
│   └── mcw-updater/            # Kiểm tra & áp dụng cập nhật (GitHub Releases Rust Edition — hiện đang tắt)
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
| **`mcw-updater`** | - [`src/core/update/update_manager.py`](file:///d:/WORKBASE/mcw-launcher/src/core/update/update_manager.py)<br>- [`src/core/update/github_release_client.py`](file:///d:/WORKBASE/mcw-launcher/src/core/update/github_release_client.py) | - Quy tắc kiểm tra phiên bản (`LauncherVersion`) dành riêng cho repo Rust Edition (đã gỡ bỏ hoàn toàn nhánh Cloudflare/Python, đang tắt mặc định `UPDATES_ENABLED = false`). |

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
| **`v0.6.0-rc.1` → `2.0`** | **Migration tự động từ `1.8.x`**, hoàn thiện đa ngôn ngữ EN/VI, CI/CD đóng gói Windows & Linux, tách biệt kênh phát hành riêng của Rust Edition. | Sẵn sàng phát hành chính thức **MCW Launcher 2.0 (Rust Edition)**. |

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

---

## 7. Danh Sách Đối Chiếu Parity (`v0.6.0-beta.1` Rust vs MCW Launcher Python)

> Bảng rà soát thực tế các tính năng, luồng nghiệp vụ và chi tiết logic **chưa làm** hoặc **chưa giống hoàn toàn** so với MCW Launcher Python (`d:\WORKBASE\mcw-launcher`), dùng làm checklist triển khai tiếp cho `v0.6.x` → `2.0`.

### 7.1. Core Launch, Runtime, Java & Mạng

**Chưa làm trong bản Rust:**
- [x] **JVM GC Presets** (`src/core/java/jvm_presets.py` → `crates/mcw-runtime/src/jvm_presets.rs`): Bộ preset cờ GC (`default`, `aikar`, `zgc`, `shenandoah`, `custom`), tự phát hiện preset (`detect_preset`) và đồng bộ 2 chiều với ô JVM arguments.
- [x] **UI chỉnh sửa JVM Arguments & Game Arguments tùy chỉnh**: Đã thêm vào tab *Instance Settings* trong `apps/desktop/src/app.rs`.
- [x] **Windows Long Command-Line Compactor** (`src/core/java/java_command_compactor.py` → `crates/mcw-runtime/src/compactor.rs`): Nén classpath dài (`META-INF/MANIFEST.MF` vào `.mcw/launch/classpath-<hash>.jar`) để tránh lỗi giới hạn 32 KB `WinError 206` trên Windows với các modpack rất lớn (>300–500 mod).
- [x] **Ưu tiên GPU rời (Dedicated GPU Preference)** (`gpu_preference_manager.py` → `GpuPreferenceManager` trong `crates/mcw-runtime/src/gpu.rs` + `prefer_dedicated_gpu` trên Windows Registry / Linux env).
- [x] **Thu nhỏ Launcher khi vào game & tự focus cửa sổ game** (`game_window_activator.py` → `GameWindowActivator` trong `crates/mcw-runtime/src/window_activator.rs` + `minimize_on_launch` trong *Launcher Settings → General*).
- [x] **Giới hạn băng thông tải (`download_limit_mbps`)** (`download_bandwidth_limiter.py` → `crates/mcw-net/src/bandwidth.rs` + cài đặt trong *Launcher Settings → Downloads*; còn `DownloadJournal` khôi phục download giữa chừng là tùy chọn nâng cao).
- [x] **Java Diagnostics & Recovery Diagnostics** (`java_diagnostics_manager.py`, `java_recovery_diagnostics.py` → `JavaDiagnosticsManager`, `JavaRecoveryDiagnostics` trong `crates/mcw-runtime/src/java_diagnostics.rs`): Dò chi tiết vendor/kiến trúc/64-bit, lưu lịch sử auto-resolve và quản lý gỡ runtime đã cài trong *Launcher Settings → Runtime*.
- [x] **Startup Recovery** (`startup_recovery_manager.py` → `StartupRecoveryService::reconcile` trong `crates/mcw-storage/src/health.rs` tự dọn lock treo `.lock` và file tạm mồ côi khi mở lại launcher).

**Khác biệt logic / chưa đồng bộ hoàn toàn:**
- [x] **UI tải Java quản lý**: Đã đồng bộ đủ `[8, 17, 21, 25]` ở cả *Launcher Settings → Runtime* lẫn *Instance Settings*.

---

### 7.2. Mod Loader & OptiFine

**Chưa làm trong bản Rust:**
- [x] **Đổi phiên bản Minecraft / Mod Loader cho Instance đã tạo** (`InstanceManager.set_runtime_profile` → `InstanceStore::set_runtime_profile` + UI trong *Instance Settings*).
- [x] **Forge Preflight, Xác nhận tương thích & Chẩn đoán Forge/Quilt** (`forge_preflight_manager.py`, `compatibility_confirmation.py`, `forge_diagnostics_manager.py`, `quilt_diagnostics_manager.py` → `ForgePreflightManager` & `LoaderDiagnosticsExporter` trong `crates/mcw-minecraft/src/preflight.rs`): Kiểm tra tiền khởi chạy Forge/NeoForge, chính sách `forge_preflight_failure_policy` (`warn`/`block`/`allow`) và xuất gói chẩn đoán `.zip` Schema `2.1` ở *Overview → Actions*.
- [x] **Legacy Forge LibLoader** (`legacy_forge_installer.py`, `legacy_libloader_manager.py` → `LegacyLibLoaderManager` trong `crates/mcw-minecraft/src/legacy_libloader.rs`): Tự động trích xuất thư viện nhúng hoặc tải transitive Maven `libloader-*` có kiểm tra SHA-512 cho các bản Forge cũ (1.12.2 trở về trước).
- [x] **OptiFine Profile Installer tự động, Registry & Giao dịch Rollback** (`optifine_profile_installer.py`, `optifine_metadata_client.py`, `optifine_registry.py`, `optifine_transaction.py`, `optifine_dialog.py` → `OptiFineRegistry`, `OptiFineTransaction`, `OptiFineManager` trong `crates/mcw-minecraft/src/optifine.rs`): Đã gắn nút *Browse*, registry `.mcw/optifine.json` theo dõi & gỡ bỏ OptiFine an toàn, hỗ trợ cài mod có kiểm tra SHA-256 trên Forge và cài standalone profile có rollback trên Vanilla.

---

### 7.3. Quản lý Instance (CRUD, Phân loại, Import/Export, Repair)

**Chưa làm trong bản Rust:**
- [x] **Nhân bản (Clone / Duplicate) Instance** (`InstanceManager.clone()` → `InstanceStore::clone_instance` + modal trong *Overview*).
- [x] **UI Quản lý Favorites, Groups, Tags & Tìm kiếm/Lọc Sidebar** (`InstanceStore::set_library_metadata` + ngôi sao `★`, nhãn Group/Tag và ô tìm kiếm Sidebar).
- [x] **Import định dạng `.mcwpack` & `-MCW-Profile.zip`** (`ModpackInstaller::install_mcwpack` + hỗ trợ `-MCW-Profile.zip` không cần khóa CurseForge + trích xuất ảnh icon nhúng `.mcw/instance-icon.*`).
- [x] **Instance Health Manager, Verification Cache & Repair Center** (`InstanceHealthManager` trong `crates/mcw-storage/src/health.rs` + `VerificationCache` trong `crates/mcw-net` + cửa sổ *Repair Center* trong `stage4_dialogs.rs`).

**Khác biệt logic / chưa đồng bộ hoàn toàn:**
- [x] **Instance Icon nâng cao** (`instance_artwork_manager.py` → `Instance::icon_origin`, `InstanceStore::set_instance_icon_bytes_with_origin` trong `crates/mcw-storage/src/instance_store.rs` & `render_sidebar_context_menu` trong `apps/desktop/src/app/views/shell.rs`): Lưu nguồn gốc ảnh (`custom` / `embedded` / `modpack:<provider>`), tự trích xuất ảnh nhúng `.mcw/instance-icon.*` khi import archive và hỗ trợ menu chuột phải trên thẻ instance ở Sidebar.

---

### 7.4. Hệ sinh thái Mod, Modpack & Content Pack

**Chưa làm trong bản Rust:**
- [x] **Hệ thống Registry & Kiểm tra/Cập nhật Mod 1 chạm + Khóa phiên bản (Version Lock)** (`ModProvenanceRegistry` & `ModUpdateManager` trong `crates/mcw-providers/src/mod_updates.rs` + UI trong tab *Mods*).
- [x] **Bộ phân tích tương thích & xung đột Mod** (`mod_compatibility_manager.py` → `ModCompatibilityAnalyzer` trong `crates/mcw-mods/src/compat.rs` kiểm tra lệch loader, trùng `mod_id`, thiếu dependency kể cả `Jar-in-Jar` và mod bị disable, hiển thị ngay ở tab *Mods* & *Repair Center*).
- [x] **Tự động giải quyết Dependency đệ quy cho CurseForge** (`CurseForgeClient::install_mod` & `install_required_dependencies_for_file` tải đệ quy mọi mod `relationType == 3`).
- [x] **Chọn lọc các file tùy chọn (Optional Files) khi cài Modpack** (`ModpackInstaller::inspect_optional_files`, `AtLauncherClient::inspect_optional_mods`, `ModpackInstallOptions` trong `crates/mcw-providers/`): Cho phép tick chọn/bỏ chọn optional mods/files trong `.mrpack`, CurseForge `.zip` và ATLauncher ngay khi cài đặt.
- [x] **Thư viện nội dung đã cài & Kho chống trùng lặp (Content Store Deduplication)** (`installed_content_library.py` → `InstalledContentLibrary` trong `crates/mcw-mods/src/library.rs`; `content_store.py`, `shared_file_materializer.py` → `ContentStore`, `SharedFileMaterializer` trong `crates/mcw-storage/src/content_store.rs`): Thư viện nội dung hợp nhất + kho lưu `cache/content_store/` (SHA-256) chống trùng lặp bằng hardlink trong *Launcher Settings → Storage*.
- [x] **Chính sách xử lý lỗi nội dung khi Launch** (`managed_content_policy.py` → `ManagedContentPolicy` trong `crates/mcw-models/src/policy.rs`): Cấu hình `modrinth_failure_policy` / `curseforge_failure_policy` / `forge_preflight_failure_policy` (`block` / `warn` / `allow`) bằng `ComboBox` trong *Launcher Settings*.
- [x] **Quản lý CurseForge API Key ngay trong Launcher Settings** (`curseforge_config_manager.py` → `save_custom_curseforge_api_key` trong `crates/mcw-providers/src/curseforge.rs` + hộp nhập ẩn/hiện khóa ngay trong *Launcher Settings → Downloads*).
- [x] **Import file `.jar` / Resource Pack / Shader Pack cục bộ & Kéo thả (Drag-and-Drop)** trực tiếp vào cửa sổ Launcher (`Import Local .jar...`, `Import Local .zip...`, OS Drag-and-Drop `.mrpack`/`.mcwpack`/`.jar`/`.zip`).

**Khác biệt logic / chưa đồng bộ hoàn toàn:**
- [x] **FTB & ATLauncher Pack Registry** (`ftb_pack_registry.py`, `atlauncher_pack_registry.py` → `FtbPackRegistry`, `AtLauncherPackRegistry` trong `crates/mcw-providers/src/pack_registry.rs`): Tự động lưu `.mcw/ftb-pack.json` và `.mcw/atlauncher-pack.json` khi cài pack, hiển thị nguồn gốc modpack trong *Overview*.

---

### 7.5. Worlds, Screenshots, Logs & Chẩn đoán lỗi

**Chưa làm trong bản Rust:**
- [x] **World Backup, Restore & Import** (`WorldManager::backup_world`, `restore_world`, `import_world` trong `crates/mcw-minecraft/src/worlds.rs` + nút *Import World (.zip)* và *Backup (.zip)* trong tab *Worlds*).
- [x] **Tự phát hiện Crash Report & Trình diễn giải lỗi Launch thân thiện** (`LaunchErrorPresenter` & `find_latest_crash_report` trong `crates/mcw-runtime/src/diagnostics.rs` + nút *Open Latest Crash Report* trong tab *Logs*).
- [x] **Xuất gói chẩn đoán ẩn danh (Diagnostic Support Bundle)** (`DiagnosticsExporter::export_bundle` trong `crates/mcw-runtime/src/diagnostics.rs` + nút *Export Diagnostic Bundle (.zip)* trong tab *Logs*).
- [x] **Trang Global Launcher Logs** (`logs_page.py` → cửa sổ *Global Launcher Logs* tách rời ở Top Bar hỗ trợ lọc, Copy và xuất file `.log` qua `SaveFileDialog`).

---

### 7.6. Multiplayer / LAN

**Chưa làm trong bản Rust:**
- [x] **Tích hợp nhà cung cấp tunnel `e4mc`** (`LanHostingManager::prepare` trong `crates/mcw-providers/src/mod_updates.rs` + nút chọn *Manual* vs *e4mc* trong *Instance Settings*, tự động cài/bật/tắt mod `e4mc` và vô hiệu hóa `mcwifipnp` cũ).
- [x] **Hộp thoại xem Log LAN Agent trong app** (`lan_agent_log_dialog.py` → `render_lan_agent_log_window` trong `apps/desktop/src/app/views/stage5_dialogs.rs`): Cửa sổ OS độc lập ở giữa màn hình xem `mcw-lan-agent.log`, hỗ trợ Refresh, Copy, Mở thư mục và Export qua `SaveFileDialog`.

**Khác biệt logic / cần sửa cho khớp Python:**
- [x] **Giá trị `lan_auth_mode` khi tắt checkbox trong `app.rs`**: Đã đồng bộ chuẩn hóa là `"microsoft_only"` khớp với `mcw-models` và Python.
- [x] **LAN Agent Target Resolver động** (`lan_agent_target_resolver.py` → `LanAgentTargetResolver` trong `crates/mcw-minecraft/src/lan_targets.rs`): Tự động phân giải Mojang ProGuard mappings, Tiny v1/v2 (Fabric intermediary / Quilt hashed), và bảng method `.class` SRG của Forge/NeoForge theo từng phiên bản Minecraft 1.14+ và truyền thẳng qua `-Dmcw.lan.targets`.

---

### 7.7. Tài khoản & Bảo mật

**Chưa làm trong bản Rust:**
- [x] **Hiển thị Avatar / Skin 2D cho tài khoản** (`account_skin_manager.py`, `minecraft_skin.py` → `AccountSkinManager` trong `crates/mcw-auth/src/skin.rs` + `apps/desktop/src/skin_render.rs` với đầu Steve/Alex nhúng sẵn, lọc mũ `(40,8,8,8)` + Nearest-Neighbor hiển thị ở Top Bar & cửa sổ Accounts).
- [x] **Kiểm tra toàn vẹn dữ liệu tài khoản (Account Integrity Manifest)** (`account_integrity.py` → `AccountIntegrity` HMAC-SHA256 + khóa DPAPI trong `crates/mcw-auth/src/integrity.rs` ký xác thực `accounts.json` / `accounts.sig`).

---

### 7.8. Launcher Settings, Theme, i18n, Auto-Updater & Tích hợp hệ thống

**Chưa làm trong bản Rust:**
- [x] **Giao diện Auto-Updater (Đã tắt theo yêu cầu & Gỡ sạch nhánh Python)** (`UpdateChecker` & `UpdateInstaller` trong `crates/mcw-updater/` chỉ dùng nguồn Rust Edition GitHub Releases, đã xóa hoàn toàn kênh Cloudflare/Python và đặt `UPDATES_ENABLED = false` cho đến khi phát hành kênh cập nhật riêng của Rust Edition).
- [x] **Discord Rich Presence** (`discord_rpc.py` → `DiscordRpcService` native Windows Named Pipe / Unix Socket trong `crates/mcw-runtime/src/discord_rpc.rs`, tự hiển thị trạng thái duyệt Launcher hoặc đang chơi Instance nào, kèm cấu hình bật/tắt & Client ID trong *Launcher Settings → General*).
- [x] **Trình hướng dẫn lần đầu (First-Run Setup Wizard) & Gợi ý phần cứng** (`first_run_recommendation_service.py`, `first_run_setup_dialog.py` → `FirstRunRecommendationService` trong `crates/mcw-runtime/src/first_run.rs` + cửa sổ *First-Run Setup Wizard* tự đo RAM/Java lần chạy đầu và cho phép chạy lại từ *Launcher Settings → General*).
- [x] **Hệ thống Theme mở rộng & Tùy biến màu sắc / chuyển động** (`src/core/theme/*` → `ThemeCatalog`, `apply_custom_accent`, `MotionMode` trong `apps/desktop/src/theme.rs` + `themes/mcw-default/theme.json`, nạp `themes/*.json` & `themes/*/theme.json`, đổi màu Accent `#RRGGBB` có tính độ tương phản chữ, reload nóng và chế độ giảm chuyển động `Full`/`Reduced`/`Off`).
- [x] **Tab Storage trong Launcher Settings & Dọn dẹp Cache cũ** (`storage_controller.py`, `legacy_storage_cleanup` → `StorageCleanupService` trong `crates/mcw-storage/src/cleanup.rs` + tab *Storage* đo dung lượng theo thư mục, cấu hình `unused_version_retention_days`, dọn API/temp cache an toàn và xóa cache version không còn instance tham chiếu).
- [x] **Trang About & Lưu kích thước cửa sổ (`remember_window_size`)**: Cửa sổ OS độc lập *About* trên Top Bar và tự động khôi phục kích thước cửa sổ (`window_width`, `window_height`). *(Phần Startup Splash / Unsaved-changes cảnh báo thêm là tùy chọn sau này.)*

**Khác biệt logic / cần rà soát:**
- [x] **Phủ i18n bằng `LanguageManager::tr` cho các nhãn & hộp thoại chính trong Desktop GUI**: Mọi nhãn điều hướng, nút hành động chính, cửa sổ cài đặt, LAN Agent Log, OptiFine, Policies, Optional Files, About và Repair Center đã dùng `self.t(...)` tra từ điển `lang/en-US.json` và `lang/vi-VN.json` chính thức.

---

### 7.9. Thứ Tự Ưu Tiên Đề Xuất Cho Các Phiên Bản Tiếp Theo

1. **Ưu tiên cao (Core & Trải nghiệm hằng ngày — `v0.6.x`)**:
   - Đổi MC version / Mod Loader trên instance có sẵn + Nhân bản (Clone) instance.
   - Favorites / Groups / Tags + ô tìm kiếm lọc ở Sidebar.
   - Ô chỉnh JVM / Game Arguments + JVM GC Presets (Aikar, ZGC, Shenandoah) + Windows `@argfile` compactor cho modpack lớn.
   - Import `.mcwpack` (đối xứng với Export) + sửa giá trị `lan_auth_mode` (`"microsoft_only"`).
   - Phủ `self.t(...)` cho toàn bộ chuỗi UI còn sót.
2. **Ưu tiên trung bình (Hệ sinh thái Mod & Multiplayer — `v0.7.x`)**:
   - Mod provenance registry + Kiểm tra & cập nhật Mod / Modpack + Khóa phiên bản + Dependency đệ quy cho CurseForge.
   - Tích hợp `e4mc` LAN hosting + World Backup/Restore/Import.
   - Tự phát hiện `crash-reports/*.txt` + diễn giải lỗi Launch thân thiện + Xuất gói chẩn đoán `.zip`.
3. **Hoàn thiện phát hành (`v0.8.x` → `2.0`)**:
   - Gắn `mcw-updater` vào Desktop GUI + cơ chế tự cập nhật (Windows/Linux).
   - First-Run Setup Wizard, Avatar/Skin tài khoản, Discord Rich Presence, Tab Storage & Theme JSON mở rộng.
