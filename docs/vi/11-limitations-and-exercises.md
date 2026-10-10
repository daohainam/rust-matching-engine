# 11. Giới hạn đã biết, điểm dễ vấp và bài tập

> *Bản gốc tiếng Anh: [11-limitations-and-exercises.md](../11-limitations-and-exercises.md)*

> **Gặp từ lạ?** Mọi thuật ngữ trong các chương đều được giải thích trong
> [bảng thuật ngữ](glossary.md).

Đây là dự án học tập; nhìn thẳng vào những điểm còn hạn chế sẽ hữu ích hơn việc
giả vờ rằng mọi thứ đều hoàn hảo. Các điểm dưới đây được xác định qua việc đọc
mã nguồn; những điểm được ghi chú đã được kiểm chứng bằng cách chạy chương trình.

Chương này có ba phần. **11.1** liệt kê những điểm còn chưa hoàn thiện. **11.2**
gồm các bài tập được xếp theo độ khó để bạn tự thử sức. **11.3** điểm lại những
lỗi từng xuất hiện trong các phiên bản trước: mỗi lỗi đều được sửa trong một
commit nhỏ mà bạn có thể xem lại, và một số lỗi cũng là những bài tập hay.

## 11.1 Giới hạn đã biết và những điểm dễ vấp

### Những điểm dễ vấp còn lại

| Ở đâu | Vấn đề | Mức độ | Chương |
|---|---|---|---|
| `wal.rs` | Các giới hạn về độ bền vững (durability): khi flush thất bại thì không thể khôi phục; mục ghi trong thư mục của tệp mới không được sync trên Unix; đĩa đầy có thể khiến tiến trình gặp `SIGBUS` trên Linux; `create` không bảo đảm tính nguyên tử; hư hỏng giữa log bị xử lý như đuôi log ghi dở; và khóa writer chỉ chặn các `WalWriter` khác, không chặn chương trình khác. | Trung bình nếu triển khai thực tế; có thể chấp nhận được khi dùng để học, miễn là bạn hiểu rõ các giới hạn này. | [9.6](09-write-ahead-log.md#96-những-điều-cần-lưu-ý-đọc-trước-khi-dựa-vào-wal) |
| `wal.rs` | Log theo định dạng trước phiên bản 4 không ghi lại sức chứa của engine cho đến khi `open_append` ghi tiếp vào log; nếu tự phát lại log với sức chứa không đúng, kết quả vẫn là một sổ lệnh khác. | Thấp: `replay` từ chối log như vậy thay vì đoán. | [9.6](09-write-ahead-log.md#96-những-điều-cần-lưu-ý-đọc-trước-khi-dựa-vào-wal) |
| `events.rs` | Một `EventBatch` chỉ giữ tối đa 64 sự kiện trong mỗi lần gọi. Số sự kiện còn lại vẫn được đếm, nên việc sự kiện bị loại bỏ không còn âm thầm, nhưng bạn phải kiểm tra `is_truncated()`. | Thấp: hãy kiểm tra giá trị này hoặc dùng một sink không giới hạn. | [7.4](07-events.md#74-chuyện-gì-xảy-ra-khi-có-quá-nhiều-sự-kiện) |
| `engine.rs` | Nếu sink phát sinh panic (và panic đó được bắt lại), sổ lệnh vẫn nhất quán nhưng yêu cầu chỉ được **áp dụng một phần**. | Thấp: dựng lại từ WAL; binary được build với `panic = "abort"` sẽ dừng hẳn (điều này phụ thuộc vào profile của bạn, [10.5](10-testing-and-performance.md#105-các-build-profile)). | [7.5](07-events.md#75-khi-sink-bị-panic) |
| `ringbuffer.rs` | Thứ tự bộ nhớ (memory ordering) của nó được kiểm tra trên phần cứng ARM và bằng Miri, nhưng phạm vi kiểm tra chỉ gồm những cách xen kẽ giữa các luồng thực sự xảy ra. | Thấp | [10.8](10-testing-and-performance.md#108-những-gì-test-không-bao-phủ) |
| `hashmap.rs` | Seed chỉ bảo vệ khỏi các id được tạo có chủ đích để gây va chạm khi nó còn bí mật; dùng `with_capacity_and_seed` với seed có thể đoán được sẽ làm mất lớp bảo vệ này. | Thấp: engine luôn dùng seed ngẫu nhiên. | [5.7](05-hash-map.md#57-độ-phức-tạp) |

### Giới hạn do thiết kế (chủ ý, không phải lỗi)

| Giới hạn | Chi tiết |
|---|---|
| **Số tick giá cấu hình cho từng engine** | Mặc định là 4096, tối đa 2^24 với `Engine::with_ticks`; bộ nhớ tăng thêm khoảng 48 byte cho mỗi tick. Không có logic về kích thước tick hay độ lệch giá; phía gọi tự quy đổi. |
| **Sức chứa lệnh cố định** | Được chọn trong `Engine::new`. Khi arena đầy, những lệnh cần nằm chờ trong sổ sẽ bị từ chối (lệnh khớp hết ngay thì vẫn thành công). |
| **Bốn loại lệnh** | Lệnh giới hạn (limit), immediate-or-cancel, lệnh thị trường (market) và post-only. Không có fill-or-kill, stop, iceberg hay lệnh trong ngày. Có thể giảm số lượng ngay tại chỗ, nhưng tăng số lượng hay đổi giá thì phải hủy rồi đặt lệnh mới, và như vậy mất ưu tiên. Lệnh thị trường chỉ được bảo vệ khi có đặt biên độ giá (price band). |
| **Giới hạn rủi ro áp dụng chung cho cả engine** | Một số lượng tối đa và một biên độ giá cho mọi lệnh. Biên độ được đo từ giá tốt nhất phía đối diện, nên nó không có tác dụng khi phía đó trống; không có giới hạn theo tài khoản, không có giá tham chiếu như giá của giao dịch gần nhất. |
| **Tài khoản chỉ là nhãn** | Mỗi lệnh gắn với một tài khoản, được dùng để chống tự giao dịch (self-trade prevention) và hủy hàng loạt. Không có giới hạn theo tài khoản (số lệnh đang mở, vị thế, hạn mức tín dụng), và engine không lưu danh sách lệnh của từng tài khoản, nên `cancel_account` phải duyệt toàn bộ sổ lệnh. |
| **Mỗi `Engine` chỉ có một mã giao dịch** | Muốn nhiều mã (symbol) thì chạy nhiều engine. |
| **`Engine` không tự kết nối với WAL hay bộ đệm vòng** | Việc kết nối do phía gọi đảm nhận; [`examples/pipeline.rs`](../../examples/pipeline.rs) minh họa cách làm, còn `WalRecord::apply_to`, `wal::replay` và `wal::replay_from` đảm nhiệm việc phát lại. `sequence` của WAL do phía gọi cấp và độc lập với bộ đếm riêng của engine. |
| **Định dạng WAL còn tối giản** | Phiên bản 6 có header được bảo vệ bằng checksum, chứa sức chứa và số tick của engine, các trường little-endian được ghi tường minh, checksum cho từng bản ghi và khóa writer, nhưng không có chính sách fsync riêng hay cơ chế xoay vòng log (log rotation) ([9.6](09-write-ahead-log.md#96-những-điều-cần-lưu-ý-đọc-trước-khi-dựa-vào-wal)). |
| **Snapshot làm tạm dừng việc khớp lệnh** | `Engine::snapshot` duyệt qua cả sổ lệnh trên luồng khớp lệnh, nên một checkpoint làm chậm các yêu cầu xếp sau nó. Hãy chụp chúng giữa các yêu cầu, và đừng chụp quá thường xuyên (bài tập 17). |
| **Lỗi phải được báo rõ ràng** | `assert!`/`expect` để kiểm tra các bất biến sẽ panic nếu phát hiện lỗi trong việc duy trì trạng thái sổ lệnh, ở mọi build profile. Với `panic = "abort"` (đã đặt trong release profile của repo này; binary *của bạn* cũng cần đặt profile tương tự vì Cargo bỏ qua profile của dependency), tiến trình sẽ dừng hẳn. Đây là chủ ý đối với một thành phần tài chính. |
| **`depth_snapshot` khá chậm** | Nó cấp phát một `Vec` và quét tuyến tính qua các tick (đã ghi trong mã nguồn). Chỉ dùng cho báo cáo. |

### Ghi chú về hiệu năng (không phải khiếm khuyết)

- `Order` chiếm **48 byte**: không vừa một cache line và cũng không phải ước số
  của 64, nên nhiều bản ghi nằm vắt ngang hai cache line
  ([chương 3](03-order-pool.md#33-bản-ghi-order)).
- `Order.sequence` được lưu nhưng không bao giờ được đọc. `Order.in_use` chỉ được
  đọc trong bước kiểm tra double free của `OrderPool::free`. Cả hai trường đều
  chiếm thêm bộ nhớ, nhưng hữu ích khi gỡ lỗi và kiểm toán.

## 11.2 Bài tập

Độ khó: 🟢 khởi động · 🟡 cần một buổi chiều tập trung · 🔴 một dự án thực thụ.
Bài tập được xếp theo chủ đề; trong mỗi nhóm, bài đi từ dễ đến khó. Gợi ý được
giữ ngắn gọn vì các chương đã trình bày đủ kiến thức cần thiết.

Hãy làm việc trên một nhánh riêng (`git switch -c my-exercise`) để nhánh `main`
giữ nguyên trạng thái được mô tả trong các chương, và chạy `cargo test` thường
xuyên. Bài nào cũng kết thúc bằng dòng **Hoàn thành khi** nêu một test cần viết
hoặc một kết quả cần thấy, để bạn biết mình đã xong.

### Làm quen

🟢 **1. `Engine::spread()`.** Hãy trả về `Option<u32>`: giá ask tốt nhất trừ giá
bid tốt nhất, hoặc `None` nếu một trong hai phía trống. Hãy kiểm thử phương thức
này trên ví dụ minh họa ở chương 1.

**Hoàn thành khi:** một test phát lại các bước 1 đến 5 của ví dụ minh họa và nhận
được `Some(1)` (ask 101, bid 100), đồng thời nhận được `None` cả với sổ lệnh trống
lẫn sau bước 6, khi không còn ask nào.

🟢 **2. Số lệnh ở mỗi mức giá.** `PriceLevel::count` đã có sẵn. Hãy thêm
`Engine::order_count_at(side, tick) -> u32`. *Gợi ý: làm giống `depth_at`.*

**Hoàn thành khi:** sau các bước 1 đến 5 của ví dụ minh họa,
`order_count_at(Side::Sell, 101)` bằng 2 và `order_count_at(Side::Buy, 99)` bằng
1, và một tick từ 4096 trở lên trả về 0 thay vì panic.

🟢 **3. Vẽ các hàng đợi.** [`examples/walkthrough.rs`](../../examples/walkthrough.rs)
in tổng số lượng ở mỗi mức giá. Hãy mở rộng ví dụ để nó in hàng đợi lệnh ở từng
mức, như trong các sơ đồ ở chương 1 (`[#2:3] -> [#3:4]`). *Gợi ý: lần theo
`head -> next_idx` cho đến `NIL`; bạn sẽ cần cung cấp quyền truy cập chỉ đọc vào
các lệnh ở một mức giá, chẳng hạn bằng một phương thức iterator trên `Engine`.*

**Hoàn thành khi:** sau bước 5, `cargo run --example walkthrough` in mức 101 dưới
dạng `[#2:3] -> [#3:4]`, và `cargo test` vẫn qua.

🟢 **4. Tận mắt xem sự kiện bị cắt bớt.** Hãy để 40 lệnh bán, mỗi lệnh số lượng 1,
nằm chờ ở cùng một mức giá, rồi dùng một lệnh mua số lượng 40 quét sạch chúng và
ghi sự kiện vào một `EventBatch`. In `len()` và `dropped()`, sau đó lặp lại với một
`Vec<Event>`. *So sánh với các unit test `bounded_batch_sink_reports_truncation`
và `deep_sweep_reports_every_execution`.*

**Hoàn thành khi:** batch báo `len() == 64` và `dropped() == 18`, còn `Vec` giữ đủ
cả 82 sự kiện (1 lần chấp nhận lệnh, 40 giao dịch, 40 lần khớp của maker, 1 lần
khớp của taker).

### Hiểu sâu phần bên trong

🟡 **5. Thu nhỏ `Order`.** `Order` chiếm 48 byte, còn các trường của nó cộng lại
là 42 byte, nên sắp xếp lại thứ tự trường cũng không giúp được gì: 48 là bội số
kế tiếp của 8 (xem [3.3](03-order-pool.md#33-bản-ghi-order)). Cần loại bỏ một
trường; ứng viên rõ nhất là `sequence`, vì không có chỗ nào đọc đến nó. Hãy bỏ
trường đó (cùng bộ đếm của engine vốn chỉ dùng để cấp giá trị cho nó), rồi sửa
test kích thước trong `pool.rs` thành 40. Sau đó thử `#[repr(C, align(64))]` trên
bản gốc, và benchmark cả hai cách so
với bản gốc. *Cách nào thắng, và kết quả có phụ thuộc vào kích thước của sổ lệnh
không?*

**Hoàn thành khi:** test kích thước chốt `size_of::<Order>()` ở mức 40, mọi test
khác vẫn qua, và bạn có số liệu benchmark cho cả ba cách bố trí.

🟡 **6. `next_set` nhanh.** Hãy thay `next_lower_active` / `next_higher_active`
(chạy tuyến tính) trong `depth_snapshot` bằng các phương thức của bitset là
`prev_set(tick)` / `next_set(tick)`. *Gợi ý: dùng phép dịch bit để che các bit tại
và thấp hơn (hoặc cao hơn) `tick` trong word chứa nó, rồi kiểm tra word đó; nếu
rỗng thì đi lên một tầng và làm tương tự với bit của word đó, cho đến khi một tầng
nào đó có bit nằm xa hơn; sau đó đi xuống lại như `lowest()` vẫn làm, lấy bit thấp
nhất (hoặc cao nhất) ở mỗi tầng.* Hãy unit test so với bản
tuyến tính cũ bằng các bitset ngẫu nhiên.

**Hoàn thành khi:** một test so sánh `next_set` / `prev_set` với các bản tuyến tính
trên hàng nghìn bitset ngẫu nhiên, và `cargo test --test model_check` vẫn qua.

🟡 **7. Seed tốn bao nhiêu?** Bảng băm trộn một seed bí mật vào mọi giá trị băm
(chương 5). Hãy viết benchmark Criterion cho `get` trên một bảng băm đã được lấp
khá đầy, rồi
làm một bản sao của `FlatHashMap` không có seed, và so sánh hai bên. *Gợi ý: đặt
bản sao trong tệp benchmark, không đặt trong `src/`, để engine vẫn giữ được lớp
bảo vệ.*

**Hoàn thành khi:** bạn có cả hai con số đo trên cùng một máy, và nói được chênh
lệch có lớn hơn độ nhiễu giữa các lần chạy mà criterion báo cáo hay không.

### Tính năng mới

🟡 **8. Lệnh fill-or-kill.** Hãy thêm `OrderType::FillOrKill`: lệnh phải khớp
toàn bộ số lượng ngay lập tức, nếu không thì không khớp gì cả. Trong trường hợp
không khớp, sổ lệnh phải còn nguyên, nên engine cần biết *trước giao dịch đầu tiên*
rằng tổng khối lượng đang chờ ở các mức giá mà lệnh chấp nhận có đủ hay không. Sau
đó, hãy quyết
định một lệnh bị hủy bỏ (killed) được báo cáo thế nào: bị từ chối trước khi được
chấp nhận (giống lệnh post-only), hay được chấp nhận rồi hết hạn (giống lệnh IOC)?
*Gợi ý: đi qua các mức giá phía đối diện, từ mức tốt nhất, cộng dồn `total_qty`, và
dừng ngay khi tổng đạt `qty` hoặc khi mức giá kế tiếp không còn khớp được nữa;
`depth_snapshot` cho thấy cách đi qua các mức giá. Bài tập 6 làm cho bước đi này
nhanh hơn. Nhớ rằng byte loại lệnh trong WAL cần một giá trị mới.*

**Hoàn thành khi:** các test cho thấy một lệnh fill-or-kill khớp hết khi có đủ số
lượng; nếu không đủ, lệnh bị hủy mà không phát sinh giao dịch và độ sâu không đổi.
Đồng thời, `model_check` vẫn qua khi bộ sinh dữ liệu có cả lệnh
fill-or-kill.

🟡 **9. Biên độ quanh giao dịch gần nhất.** Biên độ giá được đo từ giá tốt nhất
phía đối diện, nên nó biến mất khi phía đó trống, và nó dịch chuyển mỗi khi chỉ một
lệnh làm thay đổi giá tốt nhất. Các sàn giao dịch thường đo nó từ một **giá tham
chiếu** (reference price), chẳng hạn giá của giao dịch gần nhất. Hãy thêm lựa chọn
đó. *Gợi ý: engine cần lưu thêm giá giao dịch gần nhất; `match_incoming` biết giá
này mỗi khi phát ra một `Trade`. Vì đây là trạng thái của engine, nó cũng phải
được đưa vào snapshot, đồng nghĩa định dạng checkpoint cần thay đổi. WAL không cần
thêm gì, vì phát lại sẽ tự tính lại giá giao dịch gần nhất.*

**Hoàn thành khi:** với biên độ 5 quanh giao dịch gần nhất ở 100, một lệnh mua ở
106 bị từ chối ngay cả khi phía ask trống, và `tests/snapshot.rs` vẫn qua.

🟡 **10. Sửa lệnh (amend): đổi giá hoặc tăng số lượng.** `reduce_order` giữ được ưu
tiên vì lệnh giảm số lượng. Hãy thêm `amend_order(id, new_price_tick, new_qty)` để
xử lý mọi thay đổi khác: lệnh giữ nguyên vị trí nếu chỉ giảm số lượng ở cùng mức
giá; trong các trường hợp còn lại, thao tác tương đương với hủy lệnh cũ rồi đặt
lệnh mới ở cuối hàng đợi, và lệnh mới thậm chí có thể khớp nếu giá thay đổi khiến
nó vượt spread. *Gợi ý: dùng `reduce_order` để xử lý trường hợp đầu tiên. Về sự kiện, hãy tuân
theo quy tắc ở mục 7.2: consumer phải có thể dựng lại sổ lệnh chỉ từ các sự kiện.
Liệu `OrderCancelled` rồi `OrderAccepted` đã đủ, hay việc sửa lệnh xứng đáng có
một sự kiện riêng? WAL cần một loại bản ghi mới.*

**Hoàn thành khi:** các test cho thấy giảm số lượng thì giữ ưu tiên, tăng số lượng
thì mất ưu tiên, và dời giá sang bên kia spread thì có giao dịch; `model_check`
cũng gửi cả các yêu cầu sửa lệnh.

🔴 **11. Danh sách lệnh theo từng tài khoản.** `cancel_account` duyệt toàn bộ sổ
lệnh, nên hủy hai lệnh của một trader cũng có thể phải quét hàng nghìn tick. Hãy
dùng các liên kết của từng lệnh trong arena để tạo một danh sách liên kết đôi
intrusive riêng cho mỗi tài khoản, tương tự hàng đợi của một mức giá, để `cancel_account`
chỉ duyệt các lệnh của tài khoản đó. *Gợi ý: mỗi `Order` cần thêm hai liên kết
`u32` (hãy để ý
điều đó ảnh hưởng thế nào đến kích thước của nó, chương 3), và engine cần một bảng
kích thước cố định ánh xạ từ tài khoản đến đầu danh sách; bảng băm phẳng ở chương
5 có thể chứa nó. Mọi chỗ mà một lệnh đi vào hay rời khỏi sổ lệnh đều phải cập nhật
danh sách: nằm chờ, khớp lệnh, hủy lệnh, giảm số lượng về 0, chống tự giao dịch.
Hãy giữ nguyên thứ tự của các lần hủy, hoặc sửa sổ lệnh tham chiếu trong
`model_check` cho khớp.*

**Hoàn thành khi:** `model_check` vẫn qua, và benchmark cho thấy thời gian
`cancel_account` của một tài khoản có vài lệnh gần như không đổi giữa sổ lệnh có
100 và 100,000 lệnh.

### Kỹ thuật và kiểm chứng

🟡 **12. Thu gọn ca lỗi.** `tests/model_check.rs` tìm lỗi bằng những chuỗi yêu cầu
ngẫu nhiên rất dài, nhưng một lỗi lại được báo cáo ở sâu bên trong một lượt chạy
20,000 bước. Hãy chuyển bộ sinh dữ liệu của nó sang
[`proptest`](https://docs.rs/proptest), để mỗi lỗi được thu gọn (shrink) thành
chuỗi yêu cầu ngắn nhất vẫn tái hiện được lỗi.

**Hoàn thành khi:** sau khi bạn cố ý làm hỏng engine (đổi `<=` thành `<` như ở mục
[6.11](06-matching-algorithm.md#611-tự-thực-hành)), proptest báo cáo một chuỗi
gây lỗi chỉ gồm vài yêu cầu.

🔴 **13. 45 nano giây đi đâu?** Trên một thang giá 300,000 tick, việc đặt một lệnh
tốn hơn khoảng 45 ns so với trên 4096 tick (10.6). Chương 10 đoán rằng phần lớn
chênh lệch đến từ cache miss trên 14 MB mức giá, chứ không phải từ hai lần quét bit
thêm vào. Hãy kiểm chứng phỏng đoán đó. *Gợi ý: thêm một benchmark trên thang giá
rộng với các giá tập trung trong một dải hẹp, giống benchmark mặc định, rồi so
sánh; sau đó dùng một profiler đếm được cache miss (`perf stat -e cache-misses`
trên Linux, VTune hoặc Windows Performance Analyzer ở nơi khác). Để có điểm dữ liệu
thứ ba, hãy thu nhỏ `PriceLevel` (bạn có cần `count` không?) và xem khoảng chênh có
hẹp lại không.*

**Hoàn thành khi:** bạn có số liệu cho giá tập trung và giá trải rộng trên cả hai
thang giá, và nói được trong hai câu rằng bao nhiêu phần của khoảng chênh là do
cache miss.

🔴 **14. Chia shard theo mã giao dịch.** Hãy chạy bốn `Engine` trên bốn luồng, mỗi
engine nhận yêu cầu từ một bộ đệm vòng riêng, định tuyến yêu cầu theo id của mã
giao dịch (symbol id). Hãy kiểm chứng rằng các sổ lệnh độc lập với nhau và thông
lượng tăng theo số shard. *Gợi ý: bắt đầu từ
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

🟡 **16. Một log mới ở mỗi checkpoint.** Sau một checkpoint, các bản ghi cũ hơn
của log chỉ còn là gánh nặng: quá trình phục hồi bỏ qua chúng (9.8). Hãy bắt đầu một
tệp log mới ở mỗi checkpoint, để các tệp cũ có thể được lưu trữ hoặc xóa đi. *Gợi
ý: đặt tên mỗi log theo số thứ tự đầu tiên mà nó chứa, ví dụ
`wal-00000000000000005001.log`; khi một checkpoint được ghi với `next_sequence` là
N, hãy finalize log hiện tại và `create` `wal-N`. Quá trình phục hồi đọc checkpoint
mới nhất rồi dùng `replay_from` để phát lại các log tiếp theo. Chỉ xóa log khi hệ
thống downstream đã nhận đủ mọi sự kiện do log đó tạo ra (9.5).*

**Hoàn thành khi:** một test ghi ba checkpoint, sập (drop mọi thứ) rồi phục hồi
đúng trạng thái sổ lệnh hiện tại, trong khi chỉ đọc checkpoint mới nhất và các
log sau checkpoint đó.

🔴 **17. Checkpoint mà không tạm dừng việc khớp lệnh.** `Engine::snapshot` chạy trên
luồng khớp lệnh và duyệt qua cả sổ lệnh. Thay vào đó, hãy chụp snapshot trên một
luồng khác, từ một engine **bóng** (shadow) nhận cùng các yêu cầu (các test pipeline
và replay cho thấy một bản sao như vậy luôn giống hệt bản gốc). *Gợi ý: cho luồng
khớp lệnh một bộ đệm vòng thứ hai (chương 8) nối sang luồng bóng, và đẩy mỗi
`WalRecord` đã ghi log vào đó; engine bóng áp dụng chúng bằng `apply_to` và, thỉnh
thoảng, ghi một checkpoint kèm số thứ tự mà nó đã đạt tới.*

**Hoàn thành khi:** một benchmark cho thấy độ trễ của luồng khớp lệnh không đổi
trong khi engine bóng ghi checkpoint, và việc phục hồi từ checkpoint của engine
bóng tái tạo được sổ lệnh của engine chính.

🔴 **18. `create` atomic.** Hãy ghi header vào `path.tmp`, sync, đổi tên thành
`path`, rồi sync thư mục cha (trên Unix). Với từng bước, hãy giải thích nếu máy sập
ngay trước bước đó thì còn lại gì trên đĩa, và vì sao bước đó cần thiết. *Gợi ý:
`write_checkpoint` trong `src/snapshot.rs` làm đúng các bước này cho các tệp
checkpoint.*

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

**Hoàn thành khi:** một test có bản ghi hỏng theo sau bởi các bản ghi hợp lệ cho
kết quả đúng theo chính sách của bạn (ví dụ trả về lỗi), thay vì âm thầm loại bỏ
các bản ghi hợp lệ; trong khi đó, đuôi bị ghi dở vẫn được phục hồi như trước.

## 11.3 Lần theo các bản sửa lỗi

Các phiên bản trước của repo này từng có lỗi thật. Mỗi lỗi được sửa trong một
commit nhỏ, tập trung; đọc các commit đó là một trong những bài học hay nhất mà
repo mang lại. Chạy `git show` trên một bản sửa sẽ cho bạn thấy lỗi, thay đổi và
test chứng minh thay đổi đó đúng.

### Bài tập đã có lời giải: hãy thử trước khi đọc bản sửa

Những bài này ban đầu là bài tập và về sau đã được hoàn thành trong mã nguồn. Để
tự thử một bài, hãy quay về phiên bản trước các bản sửa, giải bài ở đó, rồi so sánh với
commit được nêu tên:

```sh
git switch --detach a192ebc                          # the code before the first round of fixes
# ... work on the exercise, run cargo test ...
git log --oneline main --grep "caller-supplied sink" # find the fix
git show <hash>                                      # compare with your solution
git switch main                                      # come back (stash or commit your work first)
```

`a192ebc` có **API cũ**, nên các chương (vốn mô tả mã nguồn hiện tại) sẽ không khớp
hoàn toàn với nó; hãy dùng các chương để lấy ý tưởng. Các bài R7 đến R14 được giải
muộn hơn nhiều: hãy bắt đầu chúng từ commit ngay trước commit được nêu tên
(`git switch --detach <hash>~1`).

🟢 **R1. Cờ báo tràn.** Hãy làm cho batch sự kiện kích thước cố định ngày xưa ghi
nhận xem có sự kiện nào bị loại bỏ hay không. *Đã được giải bằng
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

🔴 **R5. Kiểm thử đối chiếu (differential testing).** Hãy so sánh engine với một
bản cài đặt tham chiếu đơn giản, chậm nhưng rõ ràng đúng, trên các chuỗi yêu cầu ngẫu nhiên dài.
*Đã được giải bằng `test: model-based engine check and WAL replay determinism`
(mục 10.2). Bài tập 12 là bước tiếp theo.*

🔴 **R6. Gia cố WAL.** Một header có magic number và phiên bản, checksum cho từng
bản ghi, các trường được mã hóa theo thứ tự byte little-endian để log dùng được
trên mọi nền tảng, và hàm khởi tạo để mở log ghi nối tiếp. *Đã được giải bằng
`fix(wal)!: sound, portable, crash-safe log format`. Các giới hạn còn lại đã trở
thành R11, R12 và các bài tập 18 đến 20.*

🟡 **R7. Lệnh immediate-or-cancel.** Lệnh IOC khớp phần nào được thì khớp và không
bao giờ nằm chờ trong sổ. Với phần chưa khớp, luồng sự kiện nên là gì:
`OrderCancelled`, hay `OrderRejected` kèm một `RejectReason` mới? Hãy giải thích lựa
chọn của bạn. *Đã được giải bằng `feat(engine): add immediate-or-cancel orders`,
commit này không chọn cách nào trong hai: commit message của nó giải thích vì sao
một sự kiện mới `OrderExpired` là phù hợp nhất.*

🟡 **R8. Lệnh thị trường.** Một lệnh mua ở tick 4095 vượt qua mọi ask, nhưng phần
còn lại của nó lại nằm chờ ở đó, và đó không phải là ý nghĩa của lệnh thị trường.
Hãy xây lệnh thị trường thật sự trên nền IOC. *Đã được giải bằng
`feat(engine): add market orders`; R13 thêm cơ chế bảo vệ giá mà các sàn giao dịch
thật vẫn dùng.*

🟡 **R9. Giảm số lượng ngay tại chỗ.** Hãy cho phép trader giảm số lượng của một
lệnh mà không mất ưu tiên về thời gian; giảm về không thì phải hoạt động như hủy
lệnh. *Đã được giải bằng `feat(engine): reduce a resting order without losing priority`.
Hãy để ý một loại yêu cầu mới chạm đến bao nhiêu chỗ: sự kiện, lý do từ chối, bản
ghi WAL và bộ sinh dữ liệu test. Bài tập 10 đi xa hơn.*

🔴 **R10. Chống tự giao dịch.** Hãy cho mỗi lệnh một tài khoản. Khi một taker sắp
giao dịch với lệnh đang chờ của chính tài khoản đó, hãy áp dụng một chính sách (hủy
lệnh đang chờ, hủy lệnh mới đến, hoặc hủy cả hai) và phát ra các sự kiện tương ứng.
*Đã được giải bằng `feat(engine): give every order an account` và
`feat(engine): add self-trade prevention`. Hãy xem phần kiểm tra nằm ở đâu trong
vòng lặp bên trong của `match_incoming`, và cách maker bị hủy được loại khỏi sổ
bằng đúng các bước như khi khớp hết. Hủy hàng loạt, tính năng cũng cần đến tài
khoản, đến ngay sau đó trong
`feat(engine): cancel every order of an account, or the whole book`; bài tập 11 làm
cho nó nhanh hơn.*

🟡 **R11. Ghi lại sức chứa.** Việc phát lại chỉ tất định khi dùng cùng sức chứa
`Engine` (mục 9.1). Hãy lưu sức chứa đó vào header của log, và làm cho replay từ
chối một log được ghi cho sức chứa khác, trong khi vẫn đọc được các log cũ hơn.
*Đã được giải bằng `feat(wal)!: record the engine capacity in the log (format version 4)`;
sau đó `feat(wal): replay a log into a new engine` dựng bản sao (replica) từ chính
thông tin đó, nên quá trình phục hồi không thể chọn nhầm sức chứa được nữa.*

🟡 **R12. Mỗi tệp một writer.** Hãy lấy khóa độc quyền bằng `File::try_lock`, và
kiểm thử rằng writer thứ hai nhận một lỗi rõ ràng thay vì làm hỏng log. *Đã được
giải bằng `feat(wal): refuse a second writer with a lock file`. Commit này khóa một
tệp đi kèm chứ không khóa chính log: commit message giải thích vì sao trên Windows
không khóa trực tiếp tệp log. Checkpoint, nửa còn lại của câu chuyện phục hồi, đến ngay sau
đó, trong `feat(engine): snapshot and restore the book` và hai commit theo sau nó;
các bài tập 16 và 17 xây dựng trên nền đó.*

🟡 **R13. Bảo vệ giá cho lệnh thị trường.** Một lệnh thị trường quét cho đến khi
khớp hết, bất kể phải đi xa đến đâu. Hãy thêm một biên độ giá (price band): giá
giới hạn của lệnh thị trường trở thành giá tốt nhất phía đối diện lúc lệnh đến,
cộng thêm (với lệnh mua) hoặc trừ đi (với lệnh bán) `band` tick. *Đã được giải bằng
`feat(engine): add pre-trade risk limits`, commit này biến biên độ thành một giới
hạn rủi ro cho mọi lệnh và lấy mép của biên độ làm giá giới hạn cho lệnh thị
trường; bài tập 9 thì đo biên độ từ giao dịch gần nhất.*

🔴 **R14. Vượt quá 4096 tick.** Hãy tổng quát hóa bitset hai tầng thành nhiều tầng
hơn, để một engine có thể có hơn 4096 tick. *Đã được giải bằng
`refactor(bitset): size the price ladder at run time`, commit này chọn số tầng bất
kỳ lúc chạy thay vì cố định ba tầng, cùng với
`feat(engine)!: choose the number of price ticks per engine`. Hãy so sánh các ghi
chú benchmark của commit với mục 10.6, và xem bài tập 13.*

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
| `bitset.rs` | Giới hạn `TICKS` chỉ nằm trong một dòng comment. | `fix(bitset): enforce the TICKS limits at compile time` | [4.6](04-price-ladder-bitset.md#46-vì-sao-là-4096-tick-và-làm-sao-để-có-nhiều-hơn) |
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
