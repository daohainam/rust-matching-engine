# Bảng thuật ngữ

> *Bản gốc tiếng Anh: [glossary.md](../glossary.md)*

Mỗi thuật ngữ kỹ thuật dùng trong các chương đều được giải thích trong một hoặc
hai câu. Các thuật ngữ được nhóm theo chủ đề và sắp xếp theo thứ tự bảng chữ cái
dựa trên tên tiếng Anh (vì code và các chương đều dùng tên đó). Cột **Chương**
cho biết thuật ngữ được giải thích lần đầu hoặc được dùng nhiều nhất ở chương nào.

Chuyển nhanh đến: [Giao dịch](#giao-dịch) · [Cấu trúc dữ liệu và thuật toán](#cấu-trúc-dữ-liệu-và-thuật-toán) ·
[Phần cứng và hiệu năng](#phần-cứng-và-hiệu-năng) · [Rust](#rust) ·
[Lập trình đồng thời](#lập-trình-đồng-thời) · [Tệp và độ bền vững](#tệp-và-độ-bền-vững) ·
[Kiểm thử và công cụ](#kiểm-thử-và-công-cụ)

## Giao dịch

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Account (tài khoản) | Đơn vị đứng sau lệnh: một nhà giao dịch, một công ty hoặc bất kỳ đối tượng nào mà bên gọi theo dõi (`NewOrder::account`). Chống tự giao dịch và hủy hàng loạt đều dựa vào thông tin này. | [1](01-order-book-basics.md) |
| Aggressor / aggressor side (bên chủ động / phía chủ động) | Bên taker trong giao dịch và phía mua/bán của bên đó. Giao dịch được gọi là *buyer-initiated* (do bên mua khởi phát) khi người mua vượt spread. `Trade` ghi nhận thông tin này trong `taker_side`. | [1](01-order-book-basics.md) |
| Ask / offer (lệnh bán) | Lệnh **bán** đang chờ trong sổ. Ask tốt nhất là lệnh bán có giá thấp nhất. | [1](01-order-book-basics.md) |
| Bid (lệnh mua) | Lệnh **mua** đang chờ trong sổ. Bid tốt nhất là lệnh mua có giá cao nhất. | [1](01-order-book-basics.md) |
| Book (order book, sổ lệnh) | Danh sách mọi lệnh đang chờ, chia thành bid và ask và sắp xếp theo giá. | [1](01-order-book-basics.md) |
| Circuit breaker (cơ chế ngắt giao dịch) | Quy tắc của sàn: tạm dừng giao dịch khi giá biến động quá xa, quá nhanh. Chưa được cài đặt ở đây. | [1](01-order-book-basics.md) |
| Counterparty (đối tác) | Phía bên kia của một giao dịch: người bán đối với người mua, và người mua đối với người bán. | [1](01-order-book-basics.md) |
| Crossing the spread (vượt spread) | Lệnh đến có giá chạm tới mức giá tốt nhất ở phía đối diện, nên có thể khớp ngay. Lệnh như vậy gọi là *marketable* (có thể khớp ngay). | [1](01-order-book-basics.md) |
| Depth (độ sâu) | Tổng khối lượng lệnh đang chờ ở một mức giá hoặc trên nhiều mức giá. | [1](01-order-book-basics.md) |
| Exchange (sàn giao dịch) | Nơi người mua và người bán giao dịch với nhau; engine khớp lệnh là thành phần cốt lõi của sàn. | [1](01-order-book-basics.md) |
| Execution report (báo cáo khớp lệnh) | Thông điệp cho nhà giao dịch biết điều gì đã xảy ra với lệnh của họ. Ở đây, các [sự kiện](07-events.md) của engine đảm nhận vai trò này. | [7](07-events.md) |
| Fill / partial fill (khớp / khớp một phần) | Một lần khớp một phần hoặc toàn bộ khối lượng của lệnh. Sau khi khớp *một phần*, lệnh vẫn còn khối lượng dư. | [1](01-order-book-basics.md) |
| FIX | *Financial Information eXchange*, một giao thức dạng văn bản phổ biến mà nhà giao dịch dùng để gửi lệnh tới sàn. Gateway sẽ phân tích (parse) thông điệp này. | [2](02-architecture.md) |
| FOK (fill-or-kill) | Lệnh phải khớp toàn bộ ngay lập tức; nếu không, lệnh bị hủy mà không khớp phần nào. Chưa được cài đặt ở đây (bài tập ở chương 11). | [1](01-order-book-basics.md) |
| Gateway (cổng nhận lệnh) | Bộ phận của sàn nhận lệnh qua mạng, kiểm tra rồi chuyển tiếp lệnh đến engine khớp lệnh. | [2](02-architecture.md) |
| Instrument (mã giao dịch) | Đối tượng được giao dịch: một cổ phiếu, hợp đồng tương lai hoặc cặp tiền tệ. Mỗi mã có sổ lệnh riêng. | [1](01-order-book-basics.md) |
| IOC (immediate-or-cancel) | Lệnh khớp được bao nhiêu thì khớp ngay; phần dư hết hạn thay vì nằm chờ (`OrderType::ImmediateOrCancel`, được báo bằng `OrderExpired`). | [1](01-order-book-basics.md) |
| Limit order (lệnh giới hạn) | Lệnh có một mức giá giới hạn mà người đặt chấp nhận: "mua ở giá này hoặc thấp hơn", "bán ở giá này hoặc cao hơn". Phần dư của lệnh sẽ nằm chờ trong sổ. | [1](01-order-book-basics.md) |
| Liquidity (thanh khoản) | Khối lượng có thể giao dịch mà không làm giá biến động nhiều. Lệnh đang chờ *cung cấp* thanh khoản; taker *lấy đi* thanh khoản. | [1](01-order-book-basics.md) |
| Maker | Lệnh đã nằm chờ trong sổ trước khi tham gia giao dịch. | [1](01-order-book-basics.md) |
| Market data (dữ liệu thị trường) | Luồng dữ liệu công khai về các giao dịch và thay đổi của sổ lệnh do sàn công bố. | [2](02-architecture.md) |
| Mass cancel (hủy hàng loạt) | Hủy mọi lệnh đang chờ của một tài khoản (ví dụ khi mất kết nối: *cancel on disconnect*), hoặc hủy toàn bộ lệnh trong sổ (`cancel_account`, `cancel_all`). | [1](01-order-book-basics.md) |
| Market order (lệnh thị trường) | Lệnh không có giá riêng: nó khớp ở bất kỳ mức giá nào sổ lệnh đưa ra, và phần dư sẽ hết hạn. Các sàn thực tế giới hạn độ xa mà lệnh được phép quét bằng biên độ giá (price band); engine này cũng áp dụng giới hạn đó khi được cấu hình. | [1](01-order-book-basics.md) |
| Matching engine (engine khớp lệnh) | Thành phần ghép lệnh mua với lệnh bán và quyết định giá giao dịch. Toàn bộ repo này chính là engine khớp lệnh. | [1](01-order-book-basics.md) |
| Pre-trade risk check / fat finger (kiểm tra rủi ro trước giao dịch / lỗi bấm nhầm) | Bước kiểm tra lệnh trước khi cho phép giao dịch, nhằm ngăn các sai sót như nhập nhầm số lượng hoặc giá (lỗi *fat finger*). Ở đây, kiểm tra gồm giới hạn số lượng tối đa cho mỗi lệnh và biên độ giá (`RiskLimits`). | [1](01-order-book-basics.md) |
| Post-only (lệnh chỉ đặt chờ) | Lệnh giới hạn không được khớp ngay khi vào sổ: nếu giá chạm tới phía đối diện, lệnh bị từ chối. Các nhà tạo lập thị trường (market maker) dùng loại lệnh này để chắc chắn rằng họ chỉ bổ sung thanh khoản. | [1](01-order-book-basics.md) |
| Price level (mức giá) | Tất cả các lệnh đang chờ ở một mức giá trên một phía của sổ, được giữ thành một hàng đợi FIFO. | [1](01-order-book-basics.md) |
| Price band (biên độ giá) | Lệnh được phép vượt quá giá tốt nhất ở phía đối diện bao xa; vượt quá giới hạn thì bị từ chối, còn lệnh thị trường dừng ở mép biên độ. | [1](01-order-book-basics.md) |
| Price-time priority (ưu tiên giá-thời gian) | Quy tắc khớp lệnh: giá tốt hơn được khớp trước; cùng giá thì lệnh đến sớm hơn được khớp trước. | [1](01-order-book-basics.md) |
| Resting order (lệnh đang chờ) | Lệnh còn nằm chờ trong sổ vì khi vào sổ, nó chưa thể khớp hết. | [1](01-order-book-basics.md) |
| Self-trade prevention (chống tự giao dịch) | Quy tắc ngăn lệnh mua của một nhà giao dịch khớp với chính lệnh bán của họ. Chính sách của lệnh đến quyết định cách xử lý: hủy lệnh đang chờ, hủy lệnh đến hoặc hủy cả hai (`SelfTradePrevention`). | [1](01-order-book-basics.md) |
| Spread | Giá ask tốt nhất trừ giá bid tốt nhất. | [1](01-order-book-basics.md) |
| Sweep (quét qua nhiều mức giá) | Một taker khớp lần lượt qua nhiều mức giá liên tiếp. | [6](06-matching-algorithm.md) |
| Taker | Lệnh vừa đến trong một giao dịch. Nó "lấy đi" thanh khoản mà một maker để lại trong sổ. | [1](01-order-book-basics.md) |
| Tick (bước giá) | Bước giá nhỏ nhất. Engine này chỉ làm việc với tick nguyên, mặc định trong khoảng `0..=4095` và tối đa 2^24 tick cho mỗi engine (`Engine::with_ticks`); bên gọi chịu trách nhiệm quy đổi sang giá thực. | [1](01-order-book-basics.md) |
| Wash trade (giao dịch giả) | Giao dịch giữa hai lệnh của cùng một chủ sở hữu. Giao dịch này không tạo ra thay đổi thực chất, nhưng trong mắt thị trường lại trông như hoạt động thật; nếu cố ý thực hiện thì đó là hành vi thao túng bất hợp pháp. Xem chống tự giao dịch (self-trade prevention). | [1](01-order-book-basics.md) |

## Cấu trúc dữ liệu và thuật toán

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Amortised cost (chi phí khấu hao) | Chi phí trung bình của mỗi thao tác trên một chuỗi dài, dù một vài thao tác riêng lẻ chậm hơn (ví dụ khi tệp WAL phải lớn thêm). | [9](09-write-ahead-log.md) |
| Arena | Một mảng lớn được cấp phát sẵn, chứa mọi đối tượng cùng loại. Các đối tượng được gọi bằng chỉ số thay vì con trỏ. Ở đây: order pool. | [3](03-order-pool.md) |
| Avalanche (hiệu ứng tuyết lở) | Tính chất của một hàm băm tốt: đổi một bit đầu vào thì khoảng một nửa số bit đầu ra bị đổi. | [5](05-hash-map.md) |
| Backward-shift deletion (xóa bằng cách dịch lùi) | Cách xóa khỏi bảng băm địa chỉ mở mà không cần tombstone: các phần tử nằm sau ô trống được dịch lùi để lấp ô đó, miễn là chúng vẫn có thể được tìm thấy. | [5](05-hash-map.md) |
| Big-O (O(1), O(n)) | Khối lượng công việc tăng thế nào theo kích thước đầu vào. O(1) nghĩa là "công việc không đổi dù sổ lệnh lớn đến đâu"; O(n) nghĩa là "tỉ lệ với n". | [1](01-order-book-basics.md) |
| Bit-scan (quét bit) | Lệnh CPU tìm bit được đặt thấp nhất hoặc cao nhất trong một word. Rust cung cấp nó qua `trailing_zeros` / `leading_zeros`. | [4](04-price-ladder-bitset.md) |
| Bitset | Mảng bit dùng để ghi nhận trạng thái có/không của từng mục. Ở đây, mỗi tick ứng với một bit cho biết "có lệnh nào đang chờ ở mức giá này không?", cùng các tầng tóm tắt bên trên (bitset *nhiều tầng*, hierarchical bitset). | [4](04-price-ladder-bitset.md) |
| Deterministic (tất định) | Cùng đầu vào thì luôn cùng đầu ra. Không đồng hồ, số ngẫu nhiên hay thời điểm chạy của luồng nào có thể làm đổi kết quả. | [2](02-architecture.md) |
| FIFO | *First in, first out* (vào trước, ra trước): hàng đợi mà phần tử cũ nhất rời đi trước. Mỗi mức giá là một hàng đợi FIFO. | [3](03-order-pool.md) |
| Free list / free stack (danh sách / ngăn xếp ô trống) | Danh sách các ô của arena hiện không dùng. Ở đây nó là một ngăn xếp, nên ô vừa được giải phóng gần nhất sẽ được dùng lại trước. | [3](03-order-pool.md) |
| Hash function (hàm băm) | Hàm biến một khóa (order id) thành giá trị dùng để chọn ô trong bảng. | [5](05-hash-map.md) |
| Intrusive linked list (danh sách liên kết intrusive) | Danh sách liên kết có các liên kết `next` / `prev` lưu ngay trong các phần tử, nên không cần cấp phát nút riêng. | [3](03-order-pool.md) |
| Invariant (bất biến) | Điều phải đúng tại những thời điểm nhất định, ví dụ "bit của một tick được đặt khi và chỉ khi mức giá đó không rỗng". | [6](06-matching-algorithm.md) |
| Linear probing (dò tuyến tính) | Quy tắc xử lý va chạm trong bảng băm địa chỉ mở: nếu một ô đã bị chiếm, thử ô kế tiếp rồi tiếp tục như vậy. | [5](05-hash-map.md) |
| Load factor (hệ số tải) | Mức độ lấp đầy của bảng băm, tính bằng số phần tử chia cho số ô. Việc dò chậm dần khi hệ số này tiến gần 1. | [5](05-hash-map.md) |
| Open addressing (địa chỉ mở) | Bảng băm lưu phần tử trực tiếp trong mảng các ô, thay vì trong một danh sách liên kết cho mỗi ô. | [5](05-hash-map.md) |
| Probe / probe chain (lần dò / chuỗi dò) | Probe là lần kiểm tra một ô khi tra cứu; probe chain là chuỗi ô cần kiểm tra trong một lần tra cứu. | [5](05-hash-map.md) |
| Seed | Một số ngẫu nhiên bí mật trộn vào hàm băm, để người ngoài không đoán được những id nào sẽ va chạm. | [5](05-hash-map.md) |
| Sentinel (giá trị đánh dấu đặc biệt) | Giá trị đặc biệt có nghĩa "ở đây không có gì", ví dụ `NIL = u32::MAX` cho "không có lệnh kế tiếp" hoặc `u64::MAX` cho "ô băm trống". | [3](03-order-pool.md) |
| SplitMix64 | Một hàm nhỏ, nhanh, trộn rất kỹ một số 64 bit. Được dùng làm hàm băm và trong checksum của WAL. | [5](05-hash-map.md) |
| Tombstone | Dấu để lại ở một ô băm đã bị xóa, để các lần tra cứu tiếp tục dò qua nó. Bảng băm này tránh được tombstone nhờ xóa bằng cách dịch lùi. | [5](05-hash-map.md) |

## Phần cứng và hiệu năng

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Alignment / padding (căn lề / byte đệm) | Phần lớn các kiểu dữ liệu phải bắt đầu ở địa chỉ là bội số của yêu cầu căn lề (*alignment*) của kiểu đó. Trình biên dịch chèn các byte *padding* không dùng đến để đáp ứng yêu cầu này. | [3](03-order-pool.md) |
| Benchmark | Chương trình đo xem một đoạn code chạy nhanh thế nào. | [10](10-testing-and-performance.md) |
| Branch prediction (dự đoán nhánh) | CPU đoán trước một câu lệnh `if` sẽ rẽ về đâu để tiếp tục làm việc; đoán sai thì mất thời gian. Nhánh dễ đoán gần như miễn phí. | [6](06-matching-algorithm.md) |
| Cache / cache line | Bộ nhớ nhỏ và nhanh bên trong CPU. Dữ liệu di chuyển giữa bộ nhớ chính và cache theo từng khối cố định gọi là *cache line*, thường là 64 byte. | [3](03-order-pool.md) |
| False sharing | Hai luồng ghi vào hai biến khác nhau nhưng tình cờ nằm chung một cache line, khiến cache line đó liên tục luân chuyển giữa các nhân CPU. Có thể khắc phục bằng padding (`CachePadded`). | [8](08-ring-buffer.md) |
| Heap allocation (cấp phát trên heap) | Yêu cầu bộ cấp phát bộ nhớ (`malloc`, `Box::new`, một `Vec` đang lớn lên) cấp phát bộ nhớ khi chương trình chạy. Việc này chậm và khó đoán, nên hot path tránh nó. | [2](02-architecture.md) |
| Hot path | Đoạn code chạy cho mọi lệnh. Nó phải là phần nhanh nhất. | [2](02-architecture.md) |
| Latency / tail latency (độ trễ / độ trễ đuôi) | Thời gian xử lý một yêu cầu / thời gian xử lý các yêu cầu chậm nhất (ví dụ 1 yêu cầu chậm nhất trong 100 yêu cầu). Sàn giao dịch quan tâm nhất đến độ trễ đuôi. | [8](08-ring-buffer.md) |
| Prefetcher (bộ nạp trước) | Phần cứng CPU nhận ra bạn đang đọc bộ nhớ theo thứ tự và nạp sớm các cache line kế tiếp. Các mảng phẳng được hưởng lợi từ nó. | [3](03-order-pool.md) |
| Throughput (thông lượng) | Số yêu cầu được xử lý mỗi giây. | [10](10-testing-and-performance.md) |

## Rust

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Associated function (hàm liên kết) | Hàm nằm trong khối `impl` nhưng không nhận `self`, được gọi dưới dạng `Type::function(...)`. | [6](06-matching-algorithm.md) |
| Borrow checker | Thành phần của trình biên dịch thực thi quy tắc của Rust: tại một thời điểm, một giá trị chỉ có thể có một tham chiếu `&mut` hoặc nhiều tham chiếu `&`. | [3](03-order-pool.md) |
| `dyn Trait` | Một giá trị được dùng thông qua một trait ngay lúc chạy, khi trình biên dịch không biết kiểu cụ thể của nó. Mỗi lời gọi đi qua một bảng nhỏ chứa các con trỏ hàm. | [6](06-matching-algorithm.md) |
| Generics / monomorphisation (kiểu tổng quát / đơn hình hóa) | `fn f<S: EventSink>(...)` dùng được với mọi `S`. Trình biên dịch *đơn hình hóa* hàm (monomorphize), tức tạo một bản sao của `f` cho mỗi kiểu cụ thể, nhờ đó lời gọi là trực tiếp và có thể được inline. | [6](06-matching-algorithm.md) |
| `#[inline]` | Gợi ý cho trình biên dịch đưa thân hàm vào vị trí gọi, qua đó loại bỏ chi phí gọi hàm. | [6](06-matching-algorithm.md) |
| `MaybeUninit<T>` | Vùng nhớ có thể chưa chứa một `T` hợp lệ. Đọc nó như một `T` trước khi ghi là hành vi không xác định. | [8](08-ring-buffer.md) |
| Panic / `panic = "abort"` | Một panic dừng luồng hiện tại vì có bug (ví dụ một `assert!` thất bại). Với `panic = "abort"` trong profile của binary cuối cùng, nó kết thúc cả tiến trình. | [3](03-order-pool.md), [10](10-testing-and-performance.md) |
| `#[repr(C)]` | Sắp các trường của struct theo đúng thứ tự khai báo, như C. Không có nó, Rust có thể đổi thứ tự các trường để tiết kiệm chỗ. | [3](03-order-pool.md) |
| `Send` / `Sync` | Các marker trait. `Send`: một giá trị có thể được chuyển sang luồng khác. `Sync`: một giá trị có thể được *chia sẻ* (`&T`) giữa các luồng. | [8](08-ring-buffer.md) |
| `?Sized` | Cho phép một tham số kiểu generic là kiểu có kích thước không biết lúc biên dịch, chẳng hạn `dyn EventSink`. | [6](06-matching-algorithm.md) |
| Trait | Tập các phương thức mà một kiểu cam kết cung cấp, giống một interface. `EventSink` là một trait. | [7](07-events.md) |
| Undefined behaviour, UB (hành vi không xác định) | Vi phạm một quy tắc mà trình biên dịch dựa vào (ví dụ đọc bộ nhớ chưa khởi tạo). Khi đó chương trình có thể làm *bất cứ điều gì*; đây luôn là bug. | [8](08-ring-buffer.md) |
| `unsafe` | Khối mà lập trình viên, chứ không phải trình biên dịch, chịu trách nhiệm bảo đảm các quy tắc được tuân thủ. Mọi khối `unsafe` ở đây đều có comment `// SAFETY:` giải thích vì sao khối đó an toàn. | [8](08-ring-buffer.md) |
| `UnsafeCell<T>` | Cách duy nhất được Rust cho phép để thay đổi dữ liệu thông qua một tham chiếu dùng chung `&`. Mọi kiểu cho phép thay đổi nội bộ một cách an toàn đều được xây dựng dựa trên nó. | [8](08-ring-buffer.md) |

## Lập trình đồng thời

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Atomic (`AtomicUsize`, `AtomicBool`) | Biến mà nhiều luồng có thể đọc và ghi an toàn mà không cần khóa; mỗi thao tác diễn ra nguyên tử, tức hoàn tất trọn vẹn hoặc không diễn ra. | [8](08-ring-buffer.md) |
| Compare-and-swap, CAS (so sánh rồi đổi) | Thao tác nguyên tử chỉ đổi giá trị thành B nếu giá trị hiện tại vẫn là A; cần dùng khi nhiều luồng cùng ghi một biến. Hàng đợi SPSC không cần đến thao tác này. | [8](08-ring-buffer.md) |
| Data race (tranh chấp dữ liệu) | Hai luồng cùng lúc truy cập một vùng nhớ, trong đó ít nhất một luồng ghi, mà không có đồng bộ hóa. Trong Rust, đây là hành vi không xác định. | [8](08-ring-buffer.md) |
| Disruptor (LMAX) | Thiết kế nổi tiếng của sàn LMAX: một bộ đệm vòng được cấp phát sẵn đưa dữ liệu tới một luồng xử lý duy nhất, và luồng đó sở hữu toàn bộ trạng thái. Repo này đi theo tinh thần của thiết kế đó. | [2](02-architecture.md) |
| Lock-free (không dùng khóa) | Các luồng phối hợp bằng atomic thay vì khóa (mutex), nên không luồng nào phải đợi luồng khác nhả khóa. | [8](08-ring-buffer.md) |
| Memory ordering (`Relaxed`, `Release`, `Acquire`) (thứ tự bộ nhớ) | Quy tắc xác định khi nào các thao tác ghi của một luồng trở nên nhìn thấy được ở luồng khác. Một lệnh store `Release` ghép với một lệnh load `Acquire` bảo đảm mọi thứ được ghi trước store đều nhìn thấy được sau load. `Relaxed` không bảo đảm điều đó. | [8](08-ring-buffer.md) |
| Ring buffer (bộ đệm vòng) | Một mảng kích thước cố định dùng làm hàng đợi, trong đó vị trí đọc và vị trí ghi quay vòng về đầu mảng. | [8](08-ring-buffer.md) |
| Spin loop (vòng lặp chờ) | Chờ bằng cách kiểm tra đi kiểm tra lại một điều kiện thay vì ngủ. Phản ứng nhanh, nhưng giữ một nhân CPU luôn bận. | [8](08-ring-buffer.md) |
| SPSC | *Single producer, single consumer* (một producer, một consumer): hàng đợi có đúng một luồng ghi và một luồng đọc. Chính ràng buộc này làm nó đơn giản và nhanh. | [8](08-ring-buffer.md) |
| Thread (luồng) | Một dòng thực thi độc lập bên trong chương trình. Ở đây, gateway và engine khớp lệnh chạy trên các luồng riêng. | [2](02-architecture.md) |

## Tệp và độ bền vững

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Atomic rename (đổi tên nguyên tử) | Thay thế một tệp trong một thao tác: ghi tệp mới dưới tên tạm rồi đổi tên nó để ghi đè lên tệp cũ. Nếu xảy ra sự cố, chỉ tệp cũ hoặc tệp mới còn lại, chứ không có tệp ở trạng thái trộn lẫn. Các tệp checkpoint được ghi theo cách này. | [9](09-write-ahead-log.md#98-checkpoint-phát-lại-ít-hơn) |
| Checkpoint (điểm kiểm tra) | Snapshot được lưu vào tệp cùng với vị trí cuối trong log mà snapshot đã bao gồm, để khi phục hồi chỉ cần phát lại các yêu cầu được ghi log sau vị trí đó. | [9](09-write-ahead-log.md#98-checkpoint-phát-lại-ít-hơn) |
| Checksum (mã kiểm tra) | Giá trị nhỏ được tính từ một dãy byte và lưu kèm theo dãy đó. Nếu các byte thay đổi, checksum (rất có thể) sẽ không còn khớp. | [9](09-write-ahead-log.md) |
| CRC / CRC32 | Họ checksum chuẩn (được dùng trong tệp ZIP và Ethernet). Repo này dùng checksum dựa trên SplitMix64 để tránh thêm dependency. | [9](09-write-ahead-log.md) |
| Dirty page | Một trang (page) của tệp đã bị sửa trong bộ nhớ nhưng chưa được ghi xuống đĩa. | [9](09-write-ahead-log.md) |
| Durability (độ bền vững) | Cam kết rằng dữ liệu sẽ tồn tại sau sự cố hoặc mất điện một khi hệ thống đã báo là lưu thành công. | [9](09-write-ahead-log.md) |
| Endianness / little-endian (thứ tự byte) | Thứ tự byte của một số nhiều byte trong bộ nhớ. *Little-endian* lưu byte thấp nhất trước. WAL luôn dùng little-endian nên định dạng tệp dùng được trên mọi máy. | [9](09-write-ahead-log.md) |
| `fsync` / `sync_all` / `msync` | Các lời gọi buộc hệ điều hành ghi dữ liệu đang nằm trong bộ nhớ đệm xuống đĩa. Cho đến khi một trong số chúng thành công, mất điện có thể làm mất dữ liệu. | [9](09-write-ahead-log.md) |
| File lock (advisory / mandatory) (khóa tệp: advisory / mandatory) | Cơ chế khóa trên tệp mà các chương trình khác có thể kiểm tra. Khóa *advisory* chỉ chặn các chương trình cũng yêu cầu khóa đó (Unix); khóa *mandatory* còn chặn cả việc đọc và ghi (Windows). Bộ ghi WAL khóa một tệp `.lock` đi kèm để các tiến trình đọc không bị chặn. | [9](09-write-ahead-log.md) |
| Header / magic number | Những byte đầu tiên của một tệp, mô tả định dạng của nó. *Magic number* là một giá trị cố định dùng để nhận ra loại tệp. | [9](09-write-ahead-log.md) |
| MAC | *Message authentication code*: một checksum cần khóa bí mật, nên kẻ tấn công không thể giả mạo. Checksum của WAL **không phải** là MAC. | [9](09-write-ahead-log.md) |
| Memory-mapped file, mmap (tệp ánh xạ bộ nhớ) | Một tệp được làm cho trông như một mảng trong bộ nhớ. Ghi vào mảng là ghi vào tệp, thông qua page cache. | [9](09-write-ahead-log.md) |
| Page / page cache / page fault | Hệ điều hành quản lý bộ nhớ theo từng *page* (trang, thường 4 KiB). *Page cache* giữ các page của tệp được dùng gần đây trong RAM. *Page fault* xảy ra khi chương trình chạm vào một page chưa nằm trong RAM. | [9](09-write-ahead-log.md) |
| Replay (phát lại) | Dựng lại trạng thái bằng cách đưa các yêu cầu đã ghi log vào một engine mới. Cách này khả thi vì engine có tính tất định. | [9](09-write-ahead-log.md) |
| Snapshot (ảnh chụp trạng thái) | Bản sao toàn bộ trạng thái của engine (dung lượng, các bộ đếm, các lệnh đang chờ theo thứ tự ưu tiên), đủ để dựng lại engine giống hệt. Đừng nhầm với `depth_snapshot`, hàm chỉ liệt kê các mức giá. | [9](09-write-ahead-log.md#98-checkpoint-phát-lại-ít-hơn) |
| `SIGBUS` | Tín hiệu Unix kết thúc tiến trình khi tiến trình truy cập một trang mmap mà hệ thống không thể cung cấp vùng lưu trữ tương ứng (ví dụ đĩa đầy hoặc tệp bị tiến trình khác thu nhỏ). | [9](09-write-ahead-log.md) |
| Sparse file (tệp thưa) | Tệp có những phần chưa ghi không chiếm chỗ trên đĩa; hệ điều hành đọc chúng ra toàn số 0. | [9](09-write-ahead-log.md) |
| Syscall (lời gọi hệ thống) | Yêu cầu mà chương trình gửi tới kernel của hệ điều hành (mở tệp, ghi tệp...). Syscall chậm hơn nhiều so với lời gọi hàm thông thường. | [9](09-write-ahead-log.md) |
| Torn write / torn record (ghi dở) | Lần ghi bị gián đoạn giữa chừng do sự cố, để lại bản ghi gồm một phần dữ liệu mới và một phần dữ liệu cũ (hoặc toàn số 0). Checksum phát hiện được tình huống này. | [9](09-write-ahead-log.md) |
| Write-ahead log, WAL (nhật ký ghi trước) | Một tệp ghi lại mọi yêu cầu **trước** khi áp dụng nó, để sau sự cố có thể dựng lại trạng thái bằng cách phát lại nhật ký. | [9](09-write-ahead-log.md) |

## Kiểm thử và công cụ

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| CI | *Continuous integration* (tích hợp liên tục): một máy chủ tự động build và kiểm thử mọi thay đổi. Ở đây: GitHub Actions, xem `.github/workflows/ci.yml`. | [10](10-testing-and-performance.md) |
| Counting allocator (bộ cấp phát có đếm) | Một global allocator đếm mọi lần cấp phát, dùng trong một test để chứng minh hot path không cấp phát gì. | [10](10-testing-and-performance.md) |
| Criterion | Thư viện benchmark của Rust mà `cargo bench` dùng. | [10](10-testing-and-performance.md) |
| Differential testing (kiểm thử đối chiếu) | Chạy cùng các đầu vào ngẫu nhiên trên code thật và trên một *bản cài đặt tham chiếu* đơn giản, rồi yêu cầu kết quả của hai bên phải giống hệt nhau. | [10](10-testing-and-performance.md) |
| Doctest | Ví dụ code nằm trong tài liệu mà `cargo test` biên dịch và chạy, nên nó không thể lỗi thời. | [10](10-testing-and-performance.md) |
| Global allocator | Bộ cấp phát bộ nhớ mà chương trình Rust dùng cho mọi bộ nhớ heap. Chương trình có thể thay nó bằng `#[global_allocator]`. | [10](10-testing-and-performance.md) |
| Integration test | Test nằm trong `tests/`, chỉ dùng crate qua API công khai của nó, giống một người dùng thật. | [10](10-testing-and-performance.md) |
| loom | Công cụ chạy test đồng thời với mọi cách xen kẽ khả dĩ giữa các luồng để tìm lỗi về thứ tự bộ nhớ. Công cụ này được gợi ý trong phần bài tập. | [10](10-testing-and-performance.md) |
| LTO | *Link-time optimisation* (tối ưu hóa lúc liên kết): trình biên dịch tối ưu xuyên qua ranh giới giữa các crate khi build binary cuối cùng. | [10](10-testing-and-performance.md) |
| Miri | Trình thông dịch cho Rust, phát hiện hành vi không xác định trong code `unsafe`. CI chạy các unit test dưới nó. | [10](10-testing-and-performance.md) |
| MSRV | *Minimum supported Rust version* (phiên bản Rust tối thiểu được hỗ trợ): trình biên dịch cũ nhất mà crate cam kết build được (ở đây là 1.89, để dùng `File::try_lock`). | [10](10-testing-and-performance.md) |
| Unit test | Test nhỏ nằm cạnh mã nguồn được kiểm thử (`#[cfg(test)] mod tests`), nên có thể truy cập cả các mục private. | [10](10-testing-and-performance.md) |

**Quay lại:** [README](../../README.vi.md) · [1. Kiến thức cơ bản về sổ lệnh](01-order-book-basics.md)
