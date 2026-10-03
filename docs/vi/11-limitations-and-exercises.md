# 11. Giới hạn đã biết, điểm dễ vấp và bài tập

> *Bản gốc tiếng Anh: [11-limitations-and-exercises.md](../11-limitations-and-exercises.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Đây là một dự án để học, và học từ những chỗ còn thô ráp của nó có ích hơn nhiều
so với việc giả vờ rằng nó không có chỗ nào như vậy. Mọi điều dưới đây đều được
tìm ra bằng cách đọc code và (ở chỗ có ghi chú) được xác nhận bằng cách chạy nó.

Chương này có ba phần. **11.1** liệt kê những gì còn thô ráp. **11.2** là một bộ
bài tập xếp theo độ khó để bạn tự thử sức. **11.3** lần lại những lỗi mà các phiên
bản trước từng có: mỗi lỗi được sửa trong một commit nhỏ mà bạn có thể đọc, và một
số lỗi tự nó đã là bài tập hay.

## 11.1 Giới hạn đã biết và những điểm dễ vấp

### Những điểm dễ vấp còn lại

| Ở đâu | Vấn đề | Mức độ | Chương |
|---|---|---|---|
| `wal.rs` | Các giới hạn về độ bền vững (durability): một lần flush thất bại là kết thúc, không cứu lại được; mục thư mục (directory entry) của tệp mới không được sync trên Unix; đĩa đầy biến thành `SIGBUS` trên Linux; `create` không atomic; hư hỏng ở giữa log bị coi như một đuôi bị ghi dở; và không có gì ép buộc mỗi tệp chỉ có một writer. | Trung bình nếu triển khai thật; ổn khi dùng để học, miễn là bạn biết chúng. | [9.6](09-write-ahead-log.md#96-những-điều-cần-lưu-ý-đọc-trước-khi-dựa-vào-wal) |
| `wal.rs` + `engine.rs` | Phát lại (replay) chỉ tất định khi dùng **cùng sức chứa của `Engine`**, mà log không ghi lại thông tin này. | Trung bình: dùng sai thì bản sao (replica) sẽ lặng lẽ khác bản gốc. | [9.1](09-write-ahead-log.md#91-vì-sao-phải-ghi-log) |
| `events.rs` | Một `EventBatch` chỉ giữ 64 sự kiện cho mỗi lần gọi. Phần còn lại được đếm, nên việc mất sự kiện không còn âm thầm nữa, nhưng bạn phải kiểm tra `is_truncated()`. | Thấp: hãy kiểm tra nó, hoặc dùng một sink không giới hạn. | [7.4](07-events.md#74-chuyện-gì-xảy-ra-khi-có-quá-nhiều-sự-kiện) |
| `engine.rs` | Một sink bị panic (và panic đó bị bắt lại) để lại sổ lệnh nhất quán nhưng yêu cầu chỉ được **áp dụng một phần**. | Thấp: dựng lại từ WAL; một binary build với `panic = "abort"` thì dừng hẳn (profile của chính bạn quyết định, [10.5](10-testing-and-performance.md#105-các-build-profile)). | [7.5](07-events.md#75-khi-sink-bị-panic) |
| `ringbuffer.rs` | Thứ tự bộ nhớ (memory ordering) của nó được kiểm tra trên phần cứng ARM và bằng Miri, nhưng chỉ với những kịch bản xen kẽ giữa các luồng tình cờ xảy ra. | Thấp | [10.8](10-testing-and-performance.md#108-những-gì-test-không-bao-phủ) |
| `hashmap.rs` | Seed chỉ bảo vệ khỏi các id được tạo cố ý để gây va chạm chừng nào nó còn bí mật; dùng `with_capacity_and_seed` với một seed đoán được thì mất luôn lớp bảo vệ này. | Thấp: engine luôn dùng seed ngẫu nhiên. | [5.7](05-hash-map.md#57-độ-phức-tạp) |

### Giới hạn do thiết kế (chủ ý, không phải lỗi)

| Giới hạn | Chi tiết |
|---|---|
| **4096 tick giá** | Bị cố định bởi bitset hai tầng (`64 × 64`), và được kiểm tra lúc biên dịch. Không có logic về kích thước tick hay độ lệch giá; phía gọi tự quy đổi. |
| **Sức chứa lệnh cố định** | Được chọn trong `Engine::new`. Khi arena đầy, những lệnh cần nằm chờ trong sổ sẽ bị từ chối (lệnh khớp hết ngay thì vẫn thành công). |
| **Chỉ có lệnh giới hạn** | Không có lệnh thị trường (market), IOC, FOK, stop, iceberg; không có sửa lệnh (hủy rồi đặt lệnh mới thì mất ưu tiên); không có chống tự khớp lệnh (self-trade prevention); không có tài khoản. |
| **Mỗi `Engine` chỉ có một mã giao dịch** | Muốn nhiều mã (symbol) thì chạy nhiều engine. |
| **`Engine` không được nối sẵn với WAL hay bộ đệm vòng** | Phía gọi tự nối; [`examples/pipeline.rs`](../../examples/pipeline.rs) cho thấy cách làm. `sequence` của WAL do phía gọi cấp và độc lập với bộ đếm riêng của engine. |
| **Định dạng WAL còn nhỏ gọn** | Phiên bản 1 có header, các trường little-endian tường minh và checksum cho từng bản ghi, nhưng không có chính sách fsync riêng, không ghi lại sức chứa của engine và không khóa tệp ([9.6](09-write-ahead-log.md#96-những-điều-cần-lưu-ý-đọc-trước-khi-dựa-vào-wal)). |
| **Lỗi thì phải kêu to** | `assert!`/`expect` trên các bất biến sẽ panic khi có lỗi sổ sách, ở mọi build profile. Với `panic = "abort"` (đã đặt trong release profile của repo này, và cũng cần đặt trong profile của binary *của bạn*, vì Cargo bỏ qua profile của dependency) thì tiến trình bị dừng hẳn. Đây là chủ ý, vì đây là một thành phần tài chính. |
| **`depth_snapshot` khá chậm** | Nó cấp phát một `Vec` và quét tuyến tính qua các tick (đã ghi trong mã nguồn). Chỉ dùng cho báo cáo. |

### Ghi chú về hiệu năng (không phải khiếm khuyết)

- `Order` nặng **48 byte**: không bằng một cache line, cũng không phải ước số của
  64, nên nhiều bản ghi nằm vắt ngang hai cache line
  ([chương 3](03-order-pool.md#33-bản-ghi-order)).
- `Order.sequence` được lưu nhưng không bao giờ được đọc. `Order.in_use` chỉ được
  đọc bởi bước kiểm tra double free trong `OrderPool::free`. Cả hai đều tốn byte,
  nhưng giúp ích cho việc gỡ lỗi và kiểm toán.

## 11.2 Bài tập

Độ khó: 🟢 khởi động · 🟡 một buổi chiều chắc tay · 🔴 một dự án thực thụ. Bài tập
được xếp theo chủ đề; trong mỗi nhóm, bài đi từ dễ đến khó. Gợi ý cố ý viết ngắn,
vì nguyên liệu đã nằm sẵn trong các chương.

Hãy làm trên một branch (`git switch -c my-exercise`) để `main` giữ nguyên như các
chương mô tả, và chạy `cargo test` thường xuyên. Bài nào cũng kết thúc bằng một
dòng **Hoàn thành khi**: một test cần viết hoặc một kết quả cần thấy, để bạn biết
mình đã xong.

### Làm quen

🟢 **1. `Engine::spread()`.** Hãy trả về `Option<u32>`: ask tốt nhất trừ bid tốt
nhất, hoặc `None` nếu một trong hai phía trống. Hãy kiểm thử nó trên ví dụ minh
họa ở chương 1.

**Hoàn thành khi:** một test chạy lại các bước 1 đến 5 của ví dụ minh họa và nhận
được `Some(1)` (ask 101, bid 100), đồng thời nhận được `None` cả với sổ lệnh trống
lẫn sau bước 6, khi không còn ask nào.

🟢 **2. Số lệnh ở mỗi mức giá.** `PriceLevel::count` đã có sẵn. Hãy thêm
`Engine::order_count_at(side, tick) -> u32`. *Gợi ý: làm giống `depth_at`.*

**Hoàn thành khi:** sau các bước 1 đến 5 của ví dụ minh họa,
`order_count_at(Side::Sell, 101)` bằng 2 và `order_count_at(Side::Buy, 99)` bằng
1, và một tick từ 4096 trở lên trả về 0 thay vì panic.

🟢 **3. Vẽ các hàng đợi.** [`examples/walkthrough.rs`](../../examples/walkthrough.rs)
in ra tổng số lượng ở mỗi mức giá. Hãy mở rộng nó để in hàng đợi lệnh của từng mức,
giống các sơ đồ ở chương 1 (`[#2:3] -> [#3:4]`). *Gợi ý: đi từ `head -> next_idx`
cho đến `NIL`; bạn sẽ cần mở quyền đọc các lệnh của một mức giá ra ngoài, ví dụ
bằng một phương thức trả về iterator trên `Engine`.*

**Hoàn thành khi:** sau bước 5, `cargo run --example walkthrough` in mức 101 dưới
dạng `[#2:3] -> [#3:4]`, và `cargo test` vẫn qua.

🟢 **4. Tận mắt xem sự kiện bị cắt bớt.** Hãy cho 40 lệnh bán, mỗi lệnh số lượng 1,
nằm chờ ở cùng một mức giá, rồi quét sạch chúng bằng một lệnh mua số lượng 40 vào
một `EventBatch`, và in ra `len()` và `dropped()`. Sau đó lặp lại với một
`Vec<Event>`. *So sánh với các unit test `bounded_batch_sink_reports_truncation`
và `deep_sweep_reports_every_execution`.*

**Hoàn thành khi:** batch báo `len() == 64` và `dropped() == 18`, còn `Vec` giữ đủ
cả 82 sự kiện (1 lần chấp nhận lệnh, 40 giao dịch, 40 lần khớp của maker, 1 lần
khớp của taker).

### Hiểu sâu phần bên trong

🟡 **5. Thu nhỏ `Order`.** Giữ nguyên `#[repr(C)]`, hãy sắp xếp lại các trường (gom
các `u32` lại, rồi đặt `u8`/`bool` ở cuối) để `size_of::<Order>()` thành 40. Thêm
một test chốt cứng kích thước này. Tiếp theo, hãy bỏ `repr(C)` và kiểm tra xem
trình biên dịch có tự đạt 40 byte hay không (xem [3.3](03-order-pool.md#33-bản-ghi-order)).
Sau đó thử `#[repr(C, align(64))]` và benchmark cả hai cách so với bản gốc. *Cách
nào thắng, và kết quả có phụ thuộc vào kích thước của sổ lệnh không?*

**Hoàn thành khi:** một test chốt `size_of::<Order>()` ở mức 40, mọi test khác vẫn
qua, và bạn có số liệu benchmark cho cả ba cách bố trí.

🟡 **6. `next_set` nhanh.** Hãy thay `next_lower_active` / `next_higher_active`
(chạy tuyến tính) trong `depth_snapshot` bằng các phương thức của bitset là
`prev_set(tick)` / `next_set(tick)`. *Gợi ý: dùng phép dịch bit để che các bit tại
và thấp hơn (hoặc cao hơn) `tick` trong word chứa nó, rồi kiểm tra word đó; nếu
rỗng thì làm tương tự với word tóm tắt (summary word).* Hãy unit test so với bản
tuyến tính cũ bằng các bitset ngẫu nhiên.

**Hoàn thành khi:** một test so sánh `next_set` / `prev_set` với các bản tuyến tính
trên hàng nghìn bitset ngẫu nhiên, và `cargo test --test model_check` vẫn qua.

🟡 **7. Seed tốn bao nhiêu?** Bảng băm trộn một seed bí mật vào mọi giá trị băm
(chương 5). Hãy viết một benchmark criterion cho `get` trên một bảng khá đầy, rồi
làm một bản sao của `FlatHashMap` không có seed, và so sánh hai bên. *Gợi ý: đặt
bản sao trong tệp benchmark, không đặt trong `src/`, để engine vẫn giữ được lớp
bảo vệ.*

**Hoàn thành khi:** bạn có cả hai con số đo trên cùng một máy, và nói được chênh
lệch có lớn hơn độ nhiễu giữa các lần chạy mà criterion báo cáo hay không.

### Tính năng mới

🟡 **8. Lệnh IOC (immediate-or-cancel).** Hãy thêm một tham số time-in-force (thời
hạn hiệu lực của lệnh). Lệnh IOC khớp phần nào được thì khớp và không bao giờ nằm
chờ trong sổ. Với phần chưa khớp, luồng sự kiện nên là gì: `OrderCancelled`, hay
`OrderRejected` kèm một `RejectReason` mới? Hãy giải thích lựa chọn của bạn.
(Nhớ quy tắc ở mục 6.9: sổ lệnh phải nhất quán tại mọi lần `emit`.)

**Hoàn thành khi:** các test cho thấy một lệnh IOC khớp một phần sẽ phát ra các
giao dịch của nó rồi đến sự kiện cho phần còn lại mà bạn đã chọn, một lệnh IOC
hoàn toàn không khớp được thì không phát giao dịch nào, và trong cả hai trường hợp
`live_order_count()` không tăng.

🟡 **9. Lệnh thị trường.** Một lệnh mua với `price_tick = 4095` vượt qua mọi ask,
nhưng phần còn lại của nó sẽ *nằm chờ* ở mức 4095, và đó không phải là ý nghĩa của
lệnh thị trường. Hãy xây lệnh thị trường thật sự trên nền IOC (bài tập 8).

**Hoàn thành khi:** một lệnh mua thị trường quét qua các ask ở mọi mức giá và không
bao giờ nằm chờ, và một lệnh thị trường trên sổ trống chỉ phát ra sự kiện chấp
nhận lệnh và sự kiện cho phần còn lại.

🟡 **10. Giảm số lượng ngay tại chỗ.** Hãy cho phép trader *giảm* số lượng của một
lệnh mà không mất ưu tiên về thời gian (tăng số lượng thì vẫn phải mất ưu tiên,
giống như hủy rồi đặt lệnh mới). *Gợi ý: chỉnh `remaining_qty` và
`level.total_qty` cùng lúc, như `reduce_qty` đang làm. Giảm xuống 0 thì phải hoạt
động như hủy lệnh.*

**Hoàn thành khi:** một test cho hai lệnh nằm chờ ở cùng một mức giá, giảm số lượng
lệnh đầu tiên, và cho thấy taker vẫn khớp lệnh đầu tiên trước; `depth_at` giảm
đúng bằng lượng đã giảm, và giảm xuống 0 thì phát ra `OrderCancelled`.

🔴 **11. Chống tự khớp lệnh.** Hãy thêm `account: u32` vào `Order`. Khi một taker
sắp giao dịch với lệnh đang chờ của chính tài khoản đó, hãy áp dụng một chính sách
(hủy lệnh cũ nhất, hủy lệnh mới nhất, hoặc trừ bớt cả hai) và phát ra các sự kiện
tương ứng. *Gợi ý: phần kiểm tra thuộc về vòng lặp trong của `match_incoming`, ngay
trước chỗ sẽ phát ra một `Trade`. Hãy truyền tài khoản của taker vào, và nhớ rằng
"hủy lệnh cũ nhất" sẽ gỡ một maker, nên nó phải giữ mức giá, bitset và bảng ánh xạ
id nhất quán đúng như khi một lệnh khớp hết.*

**Hoàn thành khi:** mỗi chính sách có một test trong đó taker và maker dùng chung
một tài khoản, và `model_check` vẫn qua khi mỗi lệnh có tài khoản riêng.

### Kỹ thuật và kiểm chứng

🟡 **12. Thu gọn ca lỗi.** `tests/model_check.rs` tìm lỗi bằng những chuỗi yêu cầu
ngẫu nhiên rất dài, nhưng một lỗi lại được báo cáo ở sâu bên trong một lượt chạy
20,000 bước. Hãy chuyển bộ sinh dữ liệu của nó sang
[`proptest`](https://docs.rs/proptest), để mỗi lỗi được thu gọn (shrink) thành
chuỗi yêu cầu ngắn nhất vẫn tái hiện được lỗi.

**Hoàn thành khi:** sau khi bạn cố ý làm hỏng engine (đổi `<=` thành `<` như ở mục
[6.11](06-matching-algorithm.md#611-tự-thực-hành)), proptest báo cáo một chuỗi
gây lỗi chỉ gồm vài yêu cầu.

🔴 **13. Bitset ba tầng.** Hãy tổng quát hóa `TwoLevelBitset` thành `N` tầng `u64`
(3 tầng = 262,144 tick). Giữ cho `lowest()` và `highest()` không có vòng lặp với
một độ sâu cố định. Cho `MAX_TICKS` thay đổi theo, và cập nhật phần kiểm tra lúc
biên dịch trên `TICKS`. *Gợi ý: tầng trên cùng vẫn là một `u64`; mỗi bit giờ tóm
tắt 64 word của tầng giữa. `lowest()` trở thành ba lần quét bit: word trên cùng,
rồi word ở tầng giữa mà nó trỏ tới, rồi word ở tầng dưới. `set` và `clear` cập
nhật tối đa ba word; `clear` chỉ được xóa bit tóm tắt khi word bên dưới nó đã bằng
không.*

**Hoàn thành khi:** các test bitset hiện có đều qua với ba tầng, một test mới tìm
đúng `lowest()` và `highest()` giữa các tick gần 0, 4,096 và 262,143, và
`model_check` vẫn qua.

🔴 **14. Chia shard theo mã giao dịch.** Hãy chạy bốn `Engine` trên bốn luồng, mỗi
engine được nuôi bởi bộ đệm vòng riêng, định tuyến yêu cầu theo id của mã giao dịch
(symbol id). Hãy kiểm chứng rằng các sổ lệnh độc lập với nhau và thông lượng tăng
theo số shard. *Gợi ý: bắt đầu từ
[`examples/pipeline.rs`](../../examples/pipeline.rs). Cho gateway mỗi shard một
`Producer` và chọn nó bằng `symbol % 4`; mỗi luồng khớp lệnh giữ `Engine` riêng và
tệp WAL riêng. Để kiểm tra tính độc lập, hãy phát lại log của từng shard riêng rẽ
và so sánh với một lần chạy đơn luồng cho từng symbol.*

**Hoàn thành khi:** sổ lệnh của mỗi shard bằng kết quả phát lại đơn luồng các yêu
cầu của shard đó, và một benchmark cho thấy thông lượng với 1, 2 và 4 shard.

🔴 **15. Dùng [`loom`](https://docs.rs/loom) để model-check bộ đệm vòng.** Hãy viết
các test loom cho `push`/`pop` và cho `is_disconnected`. Sau đó làm yếu một
`Acquire` thành `Relaxed`, và xem loom tìm ra lỗi mà x86 che giấu và cả job CI chạy
trên ARM cũng có thể bỏ sót. *Gợi ý: loom cần các phiên bản riêng của những kiểu
dữ liệu đồng thời. Dưới `#[cfg(loom)]`, hãy import `AtomicUsize`, `AtomicBool`,
`Arc` và `UnsafeCell` từ `loom` thay vì từ `std`; `UnsafeCell` của loom được dùng
qua các closure `with` và `with_mut` thay vì `get()`. Hãy bọc thân mỗi test trong
`loom::model(||
...)`, và chạy với `RUSTFLAGS="--cfg loom" cargo test --release`. Giữ các test
thật nhỏ (hai hoặc ba lần push): loom thử mọi cách xen kẽ.*

**Hoàn thành khi:** các test loom qua với thứ tự bộ nhớ thật, và loom báo lỗi khi
một `Acquire` bị làm yếu thành `Relaxed`.

### Đưa WAL đến gần mức production

🟡 **16. Ghi lại sức chứa.** Việc phát lại chỉ tất định khi dùng cùng sức chứa
`Engine` (mục 9.1). Hãy lưu sức chứa đó vào các byte dự trữ trong header (định dạng
phiên bản 2), và làm cho replay từ chối một log được ghi cho sức chứa khác. Vẫn
phải đọc được các log phiên bản 1.

**Hoàn thành khi:** phát lại một log phiên bản 2 vào một engine có sức chứa khác
thì trả về một lỗi rõ ràng, và các log phiên bản 1 do các test hiện có ghi ra vẫn
đọc đúng.

🟡 **17. Mỗi tệp một writer.** Hãy lấy khóa độc quyền bằng `File::try_lock` trong
`create` và `open_append` (đã stable từ Rust 1.89, nên cần nâng `rust-version`), và
kiểm thử rằng writer thứ hai nhận một lỗi rõ ràng thay vì làm hỏng log.

**Hoàn thành khi:** một test cho thấy việc mở writer thứ hai trên một log đang được
dùng sẽ thất bại với một lỗi, và writer thứ nhất vẫn tiếp tục hoạt động.

🔴 **18. `create` atomic.** Hãy ghi header vào `path.tmp`, sync, đổi tên thành
`path`, rồi sync thư mục cha (trên Unix). Với từng bước, hãy giải thích nếu máy sập
ngay trước bước đó thì còn lại gì trên đĩa, và vì sao bước đó cần thiết.

**Hoàn thành khi:** bạn giải thích được mục đích của từng bước trong một câu, và
một test cho thấy tệp `path.tmp` còn sót lại từ một lần `create` bị gián đoạn
không làm lần `create` kế tiếp thất bại.

🔴 **19. Không còn `SIGBUS` bất ngờ.** Hãy biến việc đĩa đầy thành một `io::Error`
trả về từ `create` hoặc `append`, thay vì một vụ sập về sau: cấp phát trước các
block của tệp (dùng `posix_fallocate` qua crate `libc`, hoặc bằng cách ghi toàn số
không). Hãy đo xem việc mở rộng tệp chậm đi bao nhiêu.

**Hoàn thành khi:** trên một hệ thống tệp cố ý để rất nhỏ (ví dụ một `tmpfs` giới
hạn dung lượng trên Linux), việc làm đầy đĩa khiến `create` hoặc `append` trả về
`io::Error` thay vì giết chết tiến trình.

🔴 **20. Hư hỏng ở giữa log.** `open_append` bỏ đi mọi thứ sau bản ghi không hợp lệ
đầu tiên. Hãy thiết kế một chính sách phân biệt được đuôi bị ghi dở (chuyện bình
thường sau một vụ sập) với hư hỏng ở giữa (có những bản ghi hợp lệ *đứng sau* một
bản ghi hỏng), và quyết định mỗi trường hợp nên xử lý thế nào. *Gợi ý: hãy nhìn vào
các số thứ tự (sequence number).*

**Hoàn thành khi:** một test có một bản ghi hỏng theo sau bởi các bản ghi hợp lệ
nhận được kết quả theo chính sách của bạn (ví dụ một lỗi) thay vì lặng lẽ mất các
bản ghi hợp lệ, trong khi đuôi bị ghi dở vẫn được phục hồi như trước.

## 11.3 Lần theo các bản sửa lỗi

Các phiên bản trước của repo này từng có lỗi thật. Mỗi lỗi được sửa trong một
commit nhỏ, tập trung, và đọc các commit đó là một trong những bài học hay nhất mà
repo mang lại: chạy `git show` trên một bản sửa sẽ cho bạn thấy lỗi, thay đổi và
test chứng minh thay đổi đó đúng.

### Bài tập đã có lời giải: hãy thử trước khi đọc bản sửa

Những bài này ban đầu là bài tập, và về sau đã được làm xong trong code. Để tự thử
một bài, hãy quay về phiên bản trước các bản sửa, giải bài ở đó, rồi so sánh với
commit được nêu tên:

```sh
git switch --detach a192ebc                          # the code before the first round of fixes
# ... work on the exercise, run cargo test ...
git log --oneline main --grep "caller-supplied sink" # find the fix
git show <hash>                                      # compare with your solution
git switch main                                      # come back (stash or commit your work first)
```

`a192ebc` có **API cũ**, nên các chương (vốn mô tả code hiện tại) sẽ không khớp
hoàn toàn với nó; hãy dùng các chương để lấy ý tưởng.

🟢 **R1. Cờ báo tràn.** Hãy làm cho batch sự kiện kích thước cố định ngày xưa ghi
nhận việc có sự kiện nào bị bỏ rơi hay không. *Đã được giải bằng
`fix(events)!: stream execution reports through a caller-supplied sink`, dưới dạng
`dropped()` và `is_truncated()`.*

🟡 **R2. Đừng trả về một batch từ mọi lần gọi nữa.** Hãy để phía gọi tự chỉ định
nơi nhận sự kiện, nhờ đó không có gì bị sao chép và không có gì bị mất. *Đã được
giải trong cùng commit với R1; hãy đo sự khác biệt bằng các benchmark (mục 10.6 có
một lần chạy).*

🟡 **R3. Hàm băm có khóa (keyed hash).** Hãy cho `FlatHashMap` một seed ngẫu nhiên
riêng cho mỗi instance. *Đã được giải bằng
`fix(hashmap): seed the hash per instance to resist crafted order IDs`. Bài tập 7
đo xem nó tốn bao nhiêu.*

🔴 **R4. SPSC an toàn về kiểu.** Hãy tách bộ đệm vòng thành `Producer<T>` và
`Consumer<T>` là `Send` nhưng không phải `Clone`, với các phương thức nhận
`&mut self`. *Đã được giải bằng
`fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles`; hãy để ý cách
các doctest `compile_fail` kiểm tra sự bảo đảm này.*

🔴 **R5. Kiểm thử đối chiếu (differential testing).** Hãy so sánh engine với một sổ
lệnh tham chiếu chậm nhưng hiển nhiên đúng, trên các chuỗi yêu cầu ngẫu nhiên dài.
*Đã được giải bằng `test: model-based engine check and WAL replay determinism`
(mục 10.2). Bài tập 12 là bước tiếp theo.*

🔴 **R6. Gia cố WAL.** Một header có magic number và phiên bản, một checksum cho
mỗi bản ghi, các trường little-endian để chạy được trên mọi nền tảng, và một hàm
khởi tạo mở log để ghi nối tiếp. *Đã được giải bằng
`fix(wal)!: sound, portable, crash-safe log format`. Các giới hạn còn lại là bài
tập 16 đến 20.*

### Mọi bản sửa, kèm commit

Hãy tìm một commit theo dòng tiêu đề (subject) của nó, rồi hiển thị nó:

```sh
git log --oneline --grep "detect double frees"   # then: git show <hash>
```

| Ở đâu | Vấn đề | Được sửa bởi (subject của commit) | Chương |
|---|---|---|---|
| `wal.rs` | `finalize` gọi `set_len` khi tệp vẫn đang được ánh xạ, và Windows từ chối việc này (lỗi hệ điều hành 1224). `Drop` nuốt mất lỗi đó, nên log giữ nguyên phần padding toàn số không và trình đọc trả về thêm các bản ghi toàn số không. | `fix(wal)!: sound, portable, crash-safe log format` (giờ vùng ánh xạ được hủy trước khi cắt ngắn tệp) | [9.4](09-write-ahead-log.md#94-vòng-đời-của-writer) |
| `events.rs` | Mỗi lần gọi trả về một batch cố định 64 sự kiện, nên một taker quét qua 32 maker trở lên sẽ **âm thầm làm mất** các báo cáo. | `fix(events)!: stream execution reports through a caller-supplied sink` | [7.3](07-events.md#73-trait-eventsink) |
| `ringbuffer.rs` | Hợp đồng SPSC không được kiểm tra: `push` và `pop` nhận `&self`, nên hai producer vẫn biên dịch được bình thường và tranh chấp với nhau (hành vi không xác định). `len()` gọi từ một luồng thứ ba có thể trả về giá trị bị quấn vòng (wrap). | `fix(ringbuffer)!: enforce SPSC roles with Producer/Consumer handles` | [8.5](08-ring-buffer.md#85-an-toàn-những-điều-hệ-thống-kiểu-bắt-buộc) |
| `hashmap.rs` | Hàm băm không có phần bí mật và SplitMix64 có thể đảo ngược, nên một client có thể tạo ra các id lệnh va chạm nhau và ép các chuỗi dò dài ra. | `fix(hashmap): seed the hash per instance to resist crafted order IDs` | [5.7](05-hash-map.md#57-độ-phức-tạp) |
| `wal.rs` | Bản ghi là các byte `repr(C)` thô: thứ tự byte theo máy, 4 byte padding chưa khởi tạo, không có header, phiên bản hay checksum, nên một bản ghi bị ghi dở trông vẫn như hợp lệ. | `fix(wal)!: sound, portable, crash-safe log format` | [9.2](09-write-ahead-log.md#92-định-dạng-bản-ghi) |
| `wal.rs` | `create` cắt cụt (xóa sạch) tệp đã có và không có cách nào để ghi tiếp vào một log, nên một lần khởi động lại ngây thơ đã xóa mất chính log mà lẽ ra nó phải phát lại. | `fix(wal)!: sound, portable, crash-safe log format` (thêm `open_append`) và `feat(wal)!: refuse to overwrite an existing log in create` | [9.5](09-write-ahead-log.md#95-đọc-và-phát-lại) |
| `engine.rs` | `depth_at` và `depth_snapshot` có thể panic với các tham số bất thường. | `fix(engine): make depth queries panic-free` | [6.1](06-matching-algorithm.md#61-api-công-khai) |
| `wal.rs` | Merge `57c7c34` gộp hai bản viết lại của cùng một tệp và làm hỏng việc build. | `fix(wal): restore the PR #2 writer lost in merge 57c7c34` | |
| `engine.rs` | `OrderFilled` của maker cuối cùng được báo cáo khi mức giá đã cạn của nó vẫn còn trong bitset, nên một panic của sink bị bắt lại sẽ để lại một giá tốt nhất đã lỗi thời. | `fix(engine): update the price index before reporting a maker fill` | [6.3](06-matching-algorithm.md#63-match_incoming-trái-tim-của-engine) |
| `hashmap.rs` | `get`, `insert` và `remove` lặp vô tận trên một bảng đã đầy, và phép tính kích thước bảng có thể bị tràn số (wrap) ở bản release. | `fix(hashmap): panic instead of hanging when the table would fill`, `fix(hashmap): check the table size computation for overflow` | [5.5](05-hash-map.md#55-định-cỡ-vì-sao-bảng-không-bao-giờ-đầy) |
| `pool.rs` | Double free chỉ bị bắt bởi `debug_assert!`, và `count -= 1` bị tràn số âm thầm ở bản release. | `fix(pool): detect double frees in release builds`, `fix(pool): check the level order count for underflow` | [3.7](03-order-pool.md#37-các-kiểm-tra-phòng-thủ) |
| `bitset.rs` | Giới hạn `TICKS` chỉ nằm trong một dòng comment. | `fix(bitset): enforce the TICKS limits at compile time` | [4.6](04-price-ladder-bitset.md#46-vì-sao-là-4096-tick) |
| `wal.rs` | Trình đọc ánh xạ log lần thứ hai, và việc này có thể tranh chấp với một writer đang chạy. | `fix(wal): read logs with plain file I/O` | [9.3](09-write-ahead-log.md#93-tệp-ánh-xạ-bộ-nhớ-trong-một-phút) |
| `ringbuffer.rs` | Consumer không phân biệt được một producer đã xong với một producer chậm, nên quay vòng chờ mãi. | `feat(ringbuffer): detect a dropped producer or consumer` | [8.5](08-ring-buffer.md#phân-biệt-chưa-có-gì-với-sẽ-không-bao-giờ-có-gì-nữa) |
| docs | README và các chương mô tả một API cũ, và không có gì phát hiện ra điều đó. | `docs(readme): fix the examples for the current API and compile them` (README doctest), `docs(ch08): move the complete program into examples/pipeline.rs` (`tests/docs_sync.rs`) | [10.7](10-testing-and-performance.md#107-tích-hợp-liên-tục-ci) |

## 11.4 Đọc thêm

- **How to Build a Fast Limit Order Book** (WK Selph): bài viết kinh điển về các
  lựa chọn cấu trúc dữ liệu đằng sau một sổ lệnh nhanh.
- Bài báo kỹ thuật **The LMAX Disruptor** (Thompson và cộng sự) và blog *Mechanical
  Sympathy* của Martin Thompson: bộ đệm vòng, nguyên tắc single-writer (chỉ một bên
  được ghi), false sharing, các hiệu ứng của cache.
- **Rust Atomics and Locks** (Mara Bos): thứ tự bộ nhớ được giải thích đến nơi đến
  chốn, là kiến thức nền cho `ringbuffer.rs`.
- **The Art of Computer Programming, Vol. 3, section 6.4** (Knuth): về băm
  (hashing), gồm cả phần phân tích dò tuyến tính được trích trong chương 5.
- **"Are You Sure You Want to Use MMAP in Your Database Management System?"**
  (Crotty, Leis và Pavlo, CIDR 2022): vì sao I/O ánh xạ bộ nhớ làm cho việc xử lý
  lỗi và độ bền vững trở nên khó, cùng những vấn đề mà phần lưu ý về WAL ở mục 9.6
  mô tả.
- **"Can Applications Recover from fsync Failures?"** (Rebello và cộng sự, USENIX
  ATC 2020): chuyện gì thực sự xảy ra sau một lần flush thất bại, và vì sao thử lại
  là chưa đủ.
- **"How to Build an Exchange"** (Brian Nigito, buổi nói chuyện kỹ thuật tại Jane
  Street): các sàn giao dịch thật tổ chức engine, sequencer và log như thế nào.

**Quay lại:** [README](../../README.vi.md)
