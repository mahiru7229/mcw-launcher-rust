# MCW Launcher 2.0 (Rust Edition) — Design Notes (Core → GUI Reference)

> File này ghi lại **từng tính năng đã hoàn thiện ở tầng Core Rust** và **cách nó hoạt động / trạng thái / luồng dữ liệu nó cung cấp**, giúp giai đoạn thiết kế & xây dựng GUI (`apps/desktop` từ `v0.3`) nắm rõ chính xác Core đang có gì mà không phải đoán hay đọc lại toàn bộ code.

---

## Mốc `v0.1.0-alpha.1` — Nền tảng Core & Vanilla Launch

### 1. Portable Storage & Atomic Config (`mcw-storage`)
- **Chức năng**: Quản lý toàn bộ cây thư mục dữ liệu của launcher (`cache/`, `instances/`, `accounts/`, `config/`, `logs/`, `backups/`, `themes/`, `runtimes/`, `instances/.runtime/locks/`) và đọc/ghi file cấu hình an toàn.
- **Cách hoạt động**:
  - `LauncherPaths::portable(root)` chuẩn hóa đường dẫn tương thích cấu trúc `1.8.x`.
  - Mọi thao tác lưu JSON (`instance.json`, `settings.json`) đi qua `atomic_write_bytes` (ghi ra temp file cùng thư mục → `sync_all` → rename có retry ngắn tránh lỗi `PermissionDenied` của Windows).
  - `safe_join_relative` chặn path traversal (`..`, đường dẫn tuyệt đối, ký tự ổ đĩa).
- **Gợi ý cho GUI**: Khi hiển thị đường dẫn (Settings / Instance Detail), có thể đọc trực tiếp từ `LauncherPaths` và mở thư mục bằng nút "Open folder".

### 2. Download Engine song song & Verify SHA-1 (`mcw-net`)
- **Chức năng**: Tải file đơn lẻ và tải hàng loạt song song (`tokio` + `reqwest` dùng `rustls-tls`) kèm kiểm tra toàn vẹn.
- **Cách hoạt động**:
  - Trước khi tải, kiểm tra file đích: nếu tồn tại và pass `verify_file` thì bỏ qua ngay (`skipped = true`).
  - Hỗ trợ `expected_size == 0` (chỉ kiểm tra dung lượng khi `expected_size > 0`, luôn lấy `SHA-1` làm chuẩn để không lỗi với Maven server).
  - Tải vào file `.part`, hỗ trợ retry exponential backoff, failover mirror và resume bằng HTTP `Range` (`206` / xử lý `416`), verify xong mới rename sang file đích.
  - `download_batch` chạy tối đa 20 luồng đồng thời, trả callback tiến độ `(done, total)` của riêng đợt chạy hiện tại.
- **Gợi ý cho GUI**: Thanh tiến trình chỉ lắng nghe stream của tác vụ hiện tại; hiển thị rõ số file `done/total` và không trộn lỗi từ phiên trước.

### 3. Offline Authentication (`mcw-auth`)
- **Chức năng**: Xác thực tài khoản Offline chuẩn Minecraft.
- **Cách hoạt động**:
  - `OfflineAuth::authenticate(username)` sinh UUIDv3 từ `OfflinePlayer:<username>` (MD5, version 3, IETF variant), trả về `Authentication` (`access_token = "0"`, `user_type = "legacy"`, `uuid` không dấu gạch ngang).
  - Có sẵn `default_skin_variant(uuid)` trả `"classic"` (Steve) hoặc `"slim"` (Alex) đúng thuật toán Minecraft.
- **Gợi ý cho GUI**: Dùng `default_skin_variant` để hiển thị avatar mặc định hợp lý cho tài khoản Offline.

### 4. Java Discovery & Policy cơ bản (`mcw-runtime`)
- **Chức năng**: Tự động tìm các bản Java đã cài trên máy và chọn phiên bản phù hợp với Minecraft.
- **Cách hoạt động**:
  - `JavaMajorPolicy` quy định Java tối thiểu theo phiên bản Minecraft (`<=1.16` → 8, `1.17` → 16, `<=1.20.4` → 17, `>=1.20.5` → 21) và ưu tiên trường `javaVersion.majorVersion` từ metadata của Mojang.
  - `JavaDiscovery::discover` quét `runtimes/`, `JAVA_HOME`, `PATH`, và thư mục JDK chuẩn (`Eclipse Adoptium`, `Java`, `Microsoft`, `Zulu`, `BellSoft`, `Amazon Corretto`), chạy `java -version` để đọc major version.
  - `JavaResolver::resolve` ưu tiên đường dẫn người dùng chọn (nếu có), sau đó chọn đúng major hoặc major cao hơn tương thích (với Vanilla cho phép chạy Java cao hơn, ví dụ Java 25 cho MC 1.21.4).
- **Gợi ý cho GUI**: Trang Settings cần dropdown chọn Java tự động (Auto) hoặc chọn từ danh sách `JavaDiscovery::discover` / Browse file `java.exe`.

### 5. Vanilla Launch Pipeline (`mcw-minecraft`)
- **Chức năng**: Chuẩn bị và khởi chạy đầy đủ Minecraft Vanilla từ đầu đến cuối.
- **Cách hoạt động**:
  1. Tải & cache `version_manifest_v2.json` (có fallback đọc cache khi mất mạng).
  2. Tải & verify `<version>.json` và `client.jar`.
  3. Lọc library theo `rules` (OS/arch/features), tải song song và giải nén `natives` an toàn (chặn zip-slip, symlink, bỏ qua `META-INF`, đánh dấu `.extracted/<sha1>`).
  4. Tải `assetIndex` và tải song song toàn bộ asset objects vào `cache/assets/objects/<hh>/<hash>`.
  5. Deduplicate classpath theo Maven `(group, artifact, classifier)`, build JVM/Game args, chuẩn hóa identity offline và tự áp dụng fix multiplayer offline cho `1.16.4`/`1.16.5`.
  6. Phát `ProgressEvent` theo từng giai đoạn thật: `FetchingManifest` → `DownloadingMetadata` → `DownloadingClient` → `DownloadingLibraries` → `ExtractingNatives` → `DownloadingAssets` → `SelectingJava` → `Completed`.
- **Gợi ý cho GUI**: Ánh xạ trực tiếp `ProgressStage` lên thanh trạng thái Launch, không tự bịa % khi đang ở giai đoạn indeterminate.

---

## Mốc `v0.2.0-alpha.1` — Instance CRUD, OAuth, Fabric/Quilt, Auto Java & Supervisor

### 6. Instance CRUD & Run Lock (`mcw-storage`, `mcw-cli instance`)
- **Chức năng**: Quản lý vòng đời Instance (Liệt kê, Tạo mới, Xem chi tiết, Đổi tên, Xóa) và khóa chống chạy trùng Instance (`InstanceRunLock`).
- **Cách hoạt động**:
  - **CRUD (`InstanceStore`)**:
    - `list_instances()`: Quét `instances/`, bỏ qua thư mục `.runtime`, đọc tất cả `instance.json` hợp lệ, sắp xếp `favorite` lên trước rồi theo tên A–Z.
    - `create_instance(name, version_id, mod_loader)`: Kiểm tra tên hợp lệ trên Windows (tối đa 80 ký tự, cấm `CON`/`NUL`/ký tự đặc biệt), chặn tạo trùng tên (`InstanceAlreadyExists`), tạo thư mục `.minecraft/`, ghi atomic `instance.json` và `settings.json` mặc định.
    - `rename_instance(old_id, new_name)`: Kiểm tra instance **không đang chạy**, validate tên mới & chống trùng, đổi tên thư mục có retry Windows, cập nhật `instance_id` và `name` trong `instance.json`.
    - `delete_instance(instance_id)`: Kiểm tra instance **không đang chạy**, xóa toàn bộ thư mục instance.
  - **Run Lock (`InstanceRunLock`)**:
    - Khi chuẩn bị/chạy game, tạo file khóa nguyên tử (`create_new(true)`) tại `instances/.runtime/locks/<instance_id>.lock` chứa JSON `{ token, instance_id, state ("preparing"|"running"), launcher_pid, minecraft_pid, updated_at }`.
    - Nếu file khóa đã tồn tại nhưng tiến trình không còn sống (stale lock), hệ thống tự dọn và cấp khóa mới; nếu vẫn đang chạy sẽ trả lỗi `InstanceAlreadyRunning`.
    - Khi game spawn thành công, cập nhật `state = "running"` + `minecraft_pid`; tự động nhả khóa khi game thoát hoặc khi `InstanceRunLock` bị `Drop`.
- **Gợi ý cho GUI**:
  - Danh sách Instance hiển thị badge trạng thái thật: `Running` (dựa trên `InstanceRunLock::is_running`), `Ready`, `Finished`, `Crashed`.
  - Khi Instance đang chạy: khóa (disable) nút **Rename** và **Delete** kèm lý do rõ ràng, nút **Launch** chuyển thành **Running / Stop**.
  - Thao tác **Delete** là hành động phá hủy dữ liệu: GUI bắt buộc hiện hộp thoại xác nhận nêu rõ tên Instance và việc xóa toàn bộ `.minecraft/` (saves, mods, configs).

### 7. Microsoft OAuth (PKCE + Device Code) & Account Store (`mcw-auth`, `mcw-cli auth`)
- **Chức năng**: Đăng nhập tài khoản Microsoft bản quyền (hỗ trợ cả **Device Code** và **Browser PKCE**), quản lý danh sách tài khoản Microsoft & Offline, tự động refresh token khi launch.
- **Cách hoạt động**:
  - **Device Code Flow** (`request_device_code` → `poll_device_code`): Lấy `user_code` + `verification_uri` để hiển thị cho người dùng, tự poll theo `interval` (xử lý `authorization_pending`, `slow_down`, `expired_token`, `access_denied`).
  - **Browser PKCE Flow** (`authenticate_browser_pkce`): Sinh `code_verifier` + `S256 code_challenge` + `state` chống CSRF; **bind & lắng nghe `127.0.0.1:8400` xong trước** rồi mới gọi callback mở trình duyệt; trả HTML kèm header bảo mật (`Cache-Control: no-store`, `CSP: default-src 'none'`).
  - **Chuỗi xác thực Minecraft** (`complete_minecraft_chain`): Microsoft OAuth Token → Xbox Live (`user.auth.xboxlive.com`) → XSTS (`xsts.auth.xboxlive.com`, có thông báo thân thiện cho các mã `XErr` như thiếu Xbox profile, sai vùng, cần xác minh người lớn, tài khoản trẻ em) → Minecraft Services (`login_with_xbox`) → Minecraft Profile & Skin URL.
  - **Lưu trữ bảo mật (`AccountStore`)**:
    - Metadata tài khoản (`uuid`, `username`, `account_type`, `skin_url`, `active`) lưu atomic tại `accounts/accounts.json`.
    - Microsoft `refresh_token` lưu vào **OS Keyring** (Windows Credential Manager / Linux Secret Service) qua crate `keyring`.
    - **Không bao giờ** lưu Minecraft `access_token` ngắn hạn xuống đĩa; mỗi lần launch tài khoản Microsoft sẽ dùng `refresh_token` để lấy token mới trong RAM.
- **Gợi ý cho GUI**:
  - Màn hình đăng nhập Microsoft nên cho phép chọn **Browser Sign-In (mặc định)** hoặc **Device Code** (hiện mã code to, rõ + nút Copy + link mở trình duyệt + trạng thái đang chờ + nút Cancel).
  - Nếu XSTS trả lỗi (ví dụ chưa tạo Xbox profile), hiển thị nguyên câu thông báo thân thiện từ `AuthError::Xsts` thay vì dump JSON thô.
  - Thanh tài khoản (Account Switcher) đọc `AccountStore::list()`, hiển thị avatar từ `skin_url` (hoặc `OfflineAuth::default_skin_variant` cho tài khoản Offline).

### 8. Auto Java Provisioner (Adoptium/Temurin) & Quét Bytecode `.class` (`mcw-runtime`)
- **Chức năng**: Tự động quét mod `.jar` để nâng phiên bản Java cần thiết (đặc biệt cho Fabric/Quilt), và tự tải + cài đặt Eclipse Temurin JDK (`8 / 17 / 21 / 25`) vào `runtimes/java-{major}/` kèm verify SHA-256.
- **Cách hoạt động**:
  - **Quét Bytecode (`detect_mods_required_java` / `detect_jar_required_java`)**:
    - Đọc `fabric.mod.json` (`depends.java`) và `quilt.mod.json` (`quilt_loader.depends`).
    - Đọc 8 byte header của tối đa 12 file `.class` trong mỗi mod `.jar` (bỏ qua `module-info.class` và `META-INF/versions/`), kiểm tra magic `0xCAFEBABE`, tính `java_major = class_major - 44` (ví dụ: mod WorldPreview biên dịch bằng Java 17 trên MC 1.16.1 sẽ tự nâng yêu cầu từ Java 8 → Java 17).
  - **Adoptium Provisioner (`JavaProvisioner::ensure_managed`)**:
    - Gọi Adoptium API v3 (`/v3/assets/latest/{major}/hotspot`) lấy bản JDK mới nhất đúng OS/Arch.
    - Tải archive `.zip`, xác thực **SHA-256**, giải nén vào `runtimes/java-{major}/` (tự bóc thư mục gốc `jdk-*/`) và probe `bin/java(.exe)` để xác nhận hoạt động.
- **Gợi ý cho GUI**:
  - Khi Launch cần tải Java tự động, hiển thị rõ giai đoạn `InstallingJava` ("Đang tải Eclipse Temurin Java 17...").
  - Khi hệ thống tự nâng Java do mod yêu cầu (ví dụ 1.16.1 + Fabric → Java 17), có thể hiển thị badge giải thích ngắn trong Instance Settings ("Auto-selected Java 17 — required by installed mods").

### 9. Fabric & Quilt Mod Loader (`mcw-minecraft`, `mcw-cli`)
- **Chức năng**: Liệt kê phiên bản Fabric Loader / Quilt Loader theo từng bản Minecraft, tự chọn bản `stable` mới nhất khi người dùng để mặc định, tải & cache profile JSON, hợp nhất thư viện Maven và `mainClass` lên nền Vanilla.
- **Cách hoạt động**:
  - **Fabric Meta v2** (`https://meta.fabricmc.net/v2`) & **Quilt Meta v3** (`https://meta.quiltmc.org/v3`):
    - `ModLoaderService::list_versions(kind, game_version)` trả danh sách `LoaderVersionEntry { version, stable }`.
    - `ModLoaderService::install_and_merge` tải `/profile/json`, cache vào `cache/versions/{loader}-{loader_ver}-{mc_ver}.json` (có offline fallback khi mất mạng).
  - Chuyển từng Maven coordinate (`group:artifact:version`) thành `LibraryEntry` (cho phép `size == 0` đúng bài học #1) rồi nối vào cuối `base.libraries` để `ClasspathBuilder` tự động ưu tiên phiên bản mới hơn (ví dụ ASM của Fabric ghi đè ASM của Vanilla).
  - Giữ `client_jar` theo `base_version_id`, cập nhật `mainClass` (`KnotClient` / `Quilt`), hợp nhất `arguments` (`jvm` + `game`), và phát giai đoạn `ProgressStage::InstallingModLoader`.
- **Gợi ý cho GUI**:
  - Màn hình Tạo Instance / Cài đặt Loader: cho chọn Loader (`Vanilla` / `Fabric` / `Quilt`), gọi `ModLoaderService::list_versions` để đổ dropdown phiên bản Loader kèm đánh dấu `(Recommended / Stable)`.
  - Nếu chọn Fabric/Quilt mà không chọn phiên bản cụ thể, Core tự lấy bản stable mới nhất.

### 10. Process Supervisor (Win32 Job Object) & Live Logs (`mcw-runtime`, `mcw-cli`)
- **Chức năng**: Giám sát tiến trình Minecraft, thu luồng log trực tiếp (stdout + stderr), dừng sạch toàn bộ cây tiến trình trên Windows, phân biệt dừng chủ động vs crash, đo thời gian chơi và nhận diện lỗi `UnsupportedClassVersionError`.
- **Cách hoạt động**:
  - **Win32 Job Object (`windows-sys`)**: Gán tiến trình game vào Job Object với cờ `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Khi người dùng nhấn Stop (`ProcessControl::request_stop()`), gọi `TerminateJobObject` để đóng sạch toàn bộ cây tiến trình con (không dùng `powershell.exe Get-CimInstance`).
  - **Phân loại thoát (`ExitClassification`)**:
    - `StoppedByUser`: Người dùng chủ động dừng (qua `request_stop()` / Ctrl+C) → **không bao giờ báo nhầm Crash**.
    - `Normal`: Thoát bình thường với mã `0`.
    - `Crashed`: Thoát khác `0` khi không có yêu cầu dừng; tự động quét 200 dòng log cuối để phát hiện `UnsupportedClassVersionError` (gợi ý nâng Java).
  - **Live Logs (`LogSink`)**: Stream từng dòng `LogLine { stream: Stdout|Stderr, text }` ra subscriber (CLI / GUI Logs tab), đồng thời ghi xuống `logs/<instance_id>-latest.log` và giữ ring buffer 200 dòng cuối.
  - **Cập nhật lịch sử Instance**: Sau khi game thoát, cập nhật `total_playtime_seconds`, `last_exit_code`, `last_launch_crashed`, `last_launch_state` (`finished` / `crashed`) vào `instance.json`.
- **Gợi ý cho GUI**:
  - Tab **Logs** lắng nghe `LogSink` theo thời gian thực (tô màu nhẹ phân biệt `Stderr` vs `Stdout`, có nút Copy / Open log file).
  - Nút **Stop** gọi `ProcessControl::request_stop()` — khi nhận `ExitClassification::StoppedByUser` thì trả trạng thái Instance về `Ready/Finished`, **không** bật popup Crash Report.
  - Khi nhận `ExitClassification::Crashed`: nếu `unsupported_class_version == true`, gợi ý trực tiếp cho người dùng nâng phiên bản Java.

---

## Mốc `v0.3.0-alpha.1` — Core Mods, Providers (Modrinth/CurseForge + SQLite WAL) & Auto-Updater

### 11. Parallel Mod Scanner & Jar-in-Jar Inspector (`mcw-mods`, `mcw-cli mods`)
- **Chức năng**: Quét toàn bộ file `.jar` / `.jar.disabled` trong thư mục `mods/` song song đa luồng (`rayon`), đọc metadata của cả 4 hệ sinh thái mod loader, phát hiện Jar-in-Jar, kiểm tra tương thích loader và bật/tắt/xóa mod.
- **Cách hoạt động**:
  - `ModScanner::scan_dir(mods_dir)` dùng `rayon::par_iter` đọc đồng thời:
    - `quilt.mod.json` (Quilt), `fabric.mod.json` (Fabric), `META-INF/neoforge.mods.toml` (NeoForge), `META-INF/mods.toml` (Forge), và `mcmod.info` (Forge legacy 1.12.2).
    - Quét Jar-in-Jar trong `META-INF/jars/*.jar` và `META-INF/jarjar/*.jar` để liệt kê `embedded_mod_ids` (tránh báo thiếu dependency giả khi mod đã nhúng sẵn thư viện như Cloth Config / Fabric API modules).
    - Gọi `detect_jar_required_java` để biết từng mod yêu cầu Java tối thiểu bao nhiêu.
  - `is_loader_compatible(mod_loader, instance_loader)` hiểu rõ quy tắc tương thích (kể cả **Fabric-on-Quilt**).
  - `ModManager` hỗ trợ `set_enabled` (đổi đuôi `.jar` ↔ `.jar.disabled`) và `delete` với kiểm tra chống path traversal.
- **Gợi ý cho GUI**:
  - Tab **Mods** trong Instance Detail có thể hiển thị hàng trăm mod tức thì không khựng UI nhờ `rayon`.
  - Mỗi dòng mod hiển thị: Toggle bật/tắt, Tên + Phiên bản + Tác giả, Badge Loader, cảnh báo nhẹ nếu `!is_loader_compatible`, và nút xóa có xác nhận.

### 12. Content Providers: SQLite WAL Cache, Modrinth v2 & CurseForge API v1 (`mcw-providers`, `mcw-cli modrinth`)
- **Chức năng**: Tìm kiếm & cài đặt Mod/Modpack/Resource Pack/Shader từ **Modrinth v2** và **CurseForge API v1**, kèm bộ nhớ đệm SQLite chế độ WAL tốc độ cao.
- **Cách hoạt động**:
  - **SQLite WAL Cache (`ApiCache`)**: Mở DB tại `cache/providers/api_cache.sqlite` với `PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;`. Đọc cache hit hoàn toàn không `fsync` (bài học #5), hỗ trợ TTL tự hết hạn (120s cho tìm kiếm, 300s cho danh sách phiên bản).
  - **Modrinth v2 (`ModrinthClient`)**:
    - `search` hỗ trợ lọc theo `project_type` (`mod`, `modpack`, `resourcepack`, `shader`), `loader`, `game_version`, phân trang (`limit`/`offset`).
    - `install_with_dependencies`: tự chọn file chính tương thích, tải kèm verify SHA-1, và **đệ quy tải toàn bộ dependency có `dependency_type == "required"`** (chống vòng lặp bằng `visited_projects`).
  - **CurseForge API v1 (`CurseForgeClient`)**:
    - Hỗ trợ `search` (map đúng `classId`, `sortField`, `modLoaderType`), `get_mod_files` truyền trực tiếp `gameVersion` + `modLoaderType` (tránh sót file với mod >50 phiên bản), và `get_files_batch` tự chia lô 50 file/request.
    - Xử lý file bị tác giả tắt phân phối bên thứ ba (`downloadUrl == null`) bằng lỗi có cấu trúc `ProviderError::ManualDownloadRequired { mod_id, file_id, file_name, website_url }`.
- **Gợi ý cho GUI**:
  - Trình duyệt **Content Browser** (Mods / Resource Packs / Shaders / Modpacks) có thể chuyển mượt giữa nguồn Modrinth & CurseForge, kết quả tìm kiếm phản hồi tức thì nhờ SQLite WAL cache.
  - Khi cài từ CurseForge mà gặp `ProviderError::ManualDownloadRequired`, GUI mở **Hộp thoại tải thủ công (Manual Download Dialog)** hiển thị tên file + nút mở `website_url` trên trình duyệt.

### 13. Auto-Updater: Cloudflare CDN + GitHub Releases Failover (`mcw-updater`, `mcw-cli update`)
- **Chức năng**: Kiểm tra phiên bản mới của MCW Launcher trên kênh `stable` hoặc `beta`, so sánh SemVer/Pre-release (`alpha < beta < rc < final`) và tự động chuyển dự phòng sang GitHub Releases khi Cloudflare CDN chưa đồng bộ hoặc gặp lỗi.
- **Cách hoạt động**:
  - `UpdateChecker::check(current_version, channel)` ưu tiên tải `https://mcw-download.pages.dev/releases/manifest.json` để tránh giới hạn rate-limit GitHub API.
  - **Bài học #3**: Nếu Cloudflare CDN lỗi, trả về rỗng, hoặc không có bản phù hợp cho kênh được chọn, **cả hai kênh `stable` lẫn `beta`** đều tự động fallback sang `https://api.github.com/repos/mahiru7229/mcw-launcher/releases`.
  - Trả về `UpdateCheckResult { current_version, channel, update_available, latest: Option<ReleaseInfo> }` (có sẵn `source`, `notes`, `html_url`, `download_url`).
- **Gợi ý cho GUI**:
  - Trang Settings → Updates cho chọn kênh `Stable` / `Beta`, nút "Check for updates", hiển thị changelog (`notes`) khi `update_available == true`.

---

## Mốc `v0.4.0-alpha.1` — Native Desktop GUI (`apps/desktop`, `egui / eframe`)

### 14. Native Rust Desktop GUI (`mcw-desktop`)
- **Chức năng**: Ứng dụng desktop thuần Rust (`egui` / `eframe 0.31`, không WebView) gắn trực tiếp lên toàn bộ 9 Core crates với bố cục 2 cột Master–Detail đúng chuẩn [DESIGN.md](DESIGN.md).
- **Cách hoạt động**:
  - **Theme & Font (`theme.rs`)**: Bảng màu Slate trung tính (Dark mặc định `#111317` + công tắc chuyển Light `#F7F8FA`), màu nhấn Emerald tiết chế (`#10B981`), bo góc sắc `2–4px`, viền `1px`; tự nạp font hệ thống (`Segoe UI` / `Cascadia Mono` / `Consolas`) để hiển thị đầy đủ dấu tiếng Việt và log monospace.
  - **Không block UI thread (`state.rs`)**: Mọi tác vụ mạng/I/O dài (tải manifest, Launch/Download/Extract, Microsoft OAuth PKCE & Device Code, Modrinth search/install, cài Temurin JDK, kiểm tra cập nhật) đều chạy trên background `tokio::runtime::Runtime` và gửi `AppEvent` về UI qua `mpsc::channel` + `ctx.request_repaint()`.
  - **Bố cục (`app.rs`)**:
    - **Top utility bar**: Account Switcher (Offline + Microsoft OAuth), kiểm tra cập nhật (`mcw-updater`), công tắc Dark/Light.
    - **Sidebar trái**: Danh sách Instance kèm viền nhấn trái Emerald cho Instance đang chọn/chạy + modal `+ New Instance`.
    - **Panel phải**: Header + nút `Launch`/`Stop` + dải trạng thái tiến trình thật (`ProgressStage`) + 4 tab (**Overview** với Open Folder / Rename / Delete có xác nhận, **Mods** với quét `rayon` + Modrinth v2 search & install, **Logs** với stream live stdout/stderr, **Settings** với RAM / độ phân giải / Java selector & Temurin installer).

---

## Mốc `v0.5.0-beta.1` — Backup/Restore, Repair, Forge/NeoForge & Modpacks

### 15. Instance Backup & Safe Rollback Restore (`mcw-storage::backup`, `mcw-cli backup`)
- **Chức năng**: Tạo bản sao lưu nén `.zip` toàn bộ `.minecraft/` của Instance, liệt kê/xóa backup, và khôi phục an toàn có cơ chế rollback.
- **Cách hoạt động**:
  - Lưu trữ tại `backups/<instance_id>/backup-<epoch>-<short_id>.zip`, kèm file `mcw-backup.json` chứa metadata (`instance_id`, `created_at_epoch`, `note`, `file_count`).
  - Chặn tạo/khôi phục backup khi Instance đang chạy (`InstanceRunLock::is_running`).
  - **Khôi phục an toàn (Bài học #6)**: Giải nén & kiểm tra zip-slip vào thư mục `cache/backups/staging/stage-<uuid>` trước → chuyển `.minecraft/` hiện tại sang `.mcw-rollback-<uuid>` → swap thư mục staged vào chỗ live. Nếu bước swap lỗi, tự rollback lại bản gốc và **tuyệt đối không xóa thư mục rollback** khi khôi phục thất bại.
- **Gợi ý cho GUI**:
  - Tab **Backups** (hoặc khu vực Maintenance trong Overview): nút "Create Backup" (có ô nhập ghi chú ngắn), danh sách backup kèm dung lượng & thời gian, nút **Restore** (có hộp thoại xác nhận) và **Delete**. Khóa khi Instance đang chạy.

### 16. Instance Repair (`mcw-minecraft::repair`, `mcw-cli repair`)
- **Chức năng**: Kiểm tra toàn vẹn và tự động tải sửa toàn bộ thành phần của một Instance (`<version>.json`, `client.jar`, libraries, natives, assets).
- **Cách hoạt động**:
  - Chặn chạy khi Instance đang mở (`InstanceRunLock::is_running`).
  - Làm mới metadata Mojang + Mod Loader, đối chiếu SHA-1 của `client.jar` (xóa và tải lại nếu hỏng), xóa cờ `.extracted/` để giải nén sạch lại toàn bộ `natives`, đồng thời chạy verify SHA-1 + tải bù mọi library & asset thiếu/lỗi.
  - Phát `ProgressEvent` thật xuyên suốt và trả về `RepairReport`.
- **Gợi ý cho GUI**:
  - Đặt nút **Repair Files** trong tab Overview (khu vực Maintenance), khóa khi Instance đang chạy, hiển thị thanh tiến trình thật và tóm tắt kết quả sau khi hoàn tất.

### 17. Modpack Installer (`.mrpack` & CurseForge `.zip`) (`mcw-providers::modpack`)
- **Chức năng**: Cài đặt Modpack trực tiếp từ file `.mrpack` (Modrinth) hoặc `.zip` (CurseForge `manifest.json`), tự tạo Instance đúng phiên bản MC & Loader, tải song song toàn bộ mod kèm verify SHA-1 và giải nén `overrides/` an toàn.
- **Cách hoạt động**:
  - Tự nhận diện định dạng qua `modrinth.index.json` hoặc `manifest.json`.
  - Với `.mrpack`: bỏ qua file `client: unsupported`, tải song song tối đa 20 luồng qua `mcw-net` (hỗ trợ mirror URL), giải nén `overrides/` và `client-overrides/` có chặn zip-slip.
  - Với CurseForge `.zip`: resolve danh sách `fileID` theo lô 50 file/request; nếu có mod tắt phân phối bên thứ ba sẽ gom vào `manual_downloads` thay vì làm hỏng cả quá trình cài đặt.
- **Gợi ý cho GUI**:
  - Thêm nút **Import Modpack** cạnh `+ New Instance` trong Sidebar; nếu `report.manual_downloads` không rỗng, hiện hộp thoại danh sách file cần tải tay kèm nút mở link trình duyệt.

### 18. FTB & ATLauncher Provider Clients (`mcw-providers::ftb`, `mcw-providers::atlauncher`)
- **Chức năng**: Tra cứu danh mục modpack từ **FTB (`api.modpacks.ch`)** và **ATLauncher (`packsnew.json`)**, dùng chung bộ nhớ đệm SQLite WAL (`ApiCache`).
- **Gợi ý cho GUI**:
  - Cho phép thêm tab/bộ lọc nguồn **FTB** và **ATLauncher** trong trình duyệt Modpack ở các bản GUI tiếp theo.

### 19. Forge & NeoForge Loader Service (`mcw-minecraft::forge`, `mcw-cli loaders`)
- **Chức năng**: Tra cứu danh sách phiên bản **Forge** (`maven.minecraftforge.net`) và **NeoForge** (`maven.neoforged.net`, kể cả nhánh legacy `1.20.1`), tải Installer JAR và tách/hợp nhất `version.json` lên nền Vanilla.
- **Gợi ý cho GUI**: Mở rộng lựa chọn Loader khi tạo Instance từ `Vanilla / Fabric / Quilt` sang đầy đủ `Vanilla / Fabric / Quilt / Forge / NeoForge`.

### 20. Migration tự động từ bản Python `1.8.x` (`mcw-storage::migration`, `mcw-cli migrate`)
- **Chức năng**: Nhập toàn bộ Instance, `.minecraft/` (saves, mods, configs) và `settings.json` từ thư mục MCW Launcher Python `1.8.x` sang Rust Edition mà **không xóa hay chỉnh sửa dữ liệu gốc**, hỗ trợ `--dry-run` và chạy lặp an toàn (bỏ qua Instance đã tồn tại).
- **Gợi ý cho GUI**: Có thể thêm nút **Import from MCW 1.8.x** trong màn hình Settings hoặc First-Run Wizard.

### 21. Đa nhiệm cài đặt song song & Thanh tiến trình trực tiếp (`mcw-providers`, `mcw-desktop`)
- **Chức năng**: Cho phép bấm cài đặt nhiều Modpack, nhiều Mod (Modrinth/CurseForge) và nhiều phiên bản Temurin Java **cùng lúc** nhờ Tokio async runtime; chỉ vô hiệu hóa đúng nút của gói đang cài, giữ sáng toàn bộ nút `Install` / `Install Pack` của các gói khác.
- **Cách hoạt động**:
  - `ModpackInstaller::install_from_archive_with_progress` và `CurseForgeClient::install_modpack_with_progress` phát callback tiến độ thật (`current`, `total`, `message`) cho từng giai đoạn (tải archive, resolve file, tải song song từng mod `done/total`, giải nén `overrides/`).
  - `McwApp` quản lý danh sách `active_tasks: Vec<ActiveTask>` và tập `HashSet` theo từng ID/slug thay vì cờ khóa toàn cục.
  - Thanh `ACTIVE INSTALLS (N)` gắn cố định dưới đáy cửa sổ (`TopBottomPanel::bottom`) tự động xuất hiện khi có tác vụ đang chạy và hiển thị thanh tiến trình + phần trăm trực tiếp cho từng tác vụ.

### 22. Gắn cờ Modpack chưa đủ mod manual & Cơ chế Auto-Scan / Manual Scan (`mcw-providers`, `mcw-desktop`)
- **Chức năng**: Mọi Instance Modpack còn thiếu mod tải thủ công (`pending_manual_downloads.json`) luôn được gắn cờ **`Incomplete (N)`** trên Sidebar và biểu ngữ **`INCOMPLETE MODPACK`** ở đầu trang chi tiết; nút **Launch** chuyển thành **`Missing Mods (N)`** để chặn việc vào nhầm game thiếu mod sau khi Cancel modal (chỉ cho phép vào game trực tiếp khi đã tải đủ, hoặc bấm `Launch Anyway (Incomplete)` chủ động trong modal).
- **Luồng 2 bước & đa cửa sổ**:
  - Cửa sổ cài Modpack (`Create Instance`) **không bao giờ tự tắt** khi cài xong 1 gói để người dùng tiếp tục tương tác/cài gói khác.
  - Ở bước cài đầu tiên, launcher tải hết mọi mod có thể tải tự động và chỉ gắn cờ `Incomplete (N)` cho instance; màn hình tải mod thủ công chỉ bật lên ở lần chạy thứ 2 (khi người dùng bấm vào instance thiếu mod).
  - Hỗ trợ mở nhiều cửa sổ **Manual Downloads** cùng lúc cho nhiều instance khác nhau, mỗi cửa sổ ghi rõ **tên Instance** trên thanh tiêu đề lẫn phần đầu cửa sổ.
- **Quét tự động & thủ công**:
  - Hỗ trợ **Auto-scan** (mặc định bật, tự kiểm tra thư mục `Downloads` và `mods/` mỗi 2 giây) và **Manual Scan** (`Scan Now` / `Scan Downloads Now`).
  - [`ModpackInstaller::scan_and_import_manual_downloads`](crates/mcw-providers/src/modpack.rs) tự bỏ qua file đang tải dở (`.crdownload`, `.part`, `.tmp`, 0-byte) và khớp linh hoạt cả tên file bị trình duyệt thêm hậu tố trùng lặp như `Mod (1).jar`.

### 23. Tinh gọn Top Bar & Cửa sổ Launcher Settings riêng (`mcw-desktop`)
- **Chức năng**: Loại bỏ hoàn toàn các nút/modal `CurseForge API`, `Import 1.8.x`, `Check updates` và nút đổi theme rời khỏi thanh Top Bar; chỉ giữ lại nút **Account** và thêm nút **biểu tượng bánh răng** (vẽ vector sắc nét) ở góc trên bên phải.
- **Cửa sổ Launcher Settings**: Mở dưới dạng cửa sổ độc lập với thanh tab lớn bên trái (`Appearance`), hiện có thiết lập **Theme: Dark (Default) / Light**.
