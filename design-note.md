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
