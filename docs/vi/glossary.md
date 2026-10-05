# Bảng thuật ngữ

> *Bản gốc tiếng Anh: [glossary.md](../glossary.md)*

Mọi thuật ngữ kỹ thuật mà các chương sử dụng, mỗi thuật ngữ được giải thích trong
một hai câu. Thuật ngữ được nhóm theo chủ đề, và trong mỗi nhóm được xếp theo thứ
tự bảng chữ cái của tên tiếng Anh (vì code và các chương đều dùng tên đó). Cột
**Chương** cho biết thuật ngữ được giải thích lần đầu, hoặc được dùng nhiều nhất, ở
chương nào.

Chuyển nhanh đến: [Giao dịch](#giao-dịch) · [Cấu trúc dữ liệu và thuật toán](#cấu-trúc-dữ-liệu-và-thuật-toán) ·
[Phần cứng và hiệu năng](#phần-cứng-và-hiệu-năng) · [Rust](#rust) ·
[Lập trình đồng thời](#lập-trình-đồng-thời) · [Tệp và độ bền vững](#tệp-và-độ-bền-vững) ·
[Kiểm thử và công cụ](#kiểm-thử-và-công-cụ)

## Giao dịch

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Aggressor / aggressor side (bên chủ động / phía chủ động) | Taker của một giao dịch, và phía của nó. Một giao dịch là *buyer-initiated* (do bên mua khởi phát) khi một người mua vượt spread. `Trade` báo thông tin này qua `taker_side`. | [1](01-order-book-basics.md) |
| Ask / offer (lệnh bán) | Một lệnh **bán** đang chờ trong sổ. Ask tốt nhất là ask thấp nhất. | [1](01-order-book-basics.md) |
| Bid (lệnh mua) | Một lệnh **mua** đang chờ trong sổ. Bid tốt nhất là bid cao nhất. | [1](01-order-book-basics.md) |
| Book (order book, sổ lệnh) | Danh sách mọi lệnh đang chờ, chia thành bid và ask và sắp xếp theo giá. | [1](01-order-book-basics.md) |
| Circuit breaker (cầu dao ngắt thị trường) | Quy tắc của sàn: tạm dừng giao dịch khi giá biến động quá xa, quá nhanh. Chưa được cài đặt ở đây. | [1](01-order-book-basics.md) |
| Counterparty (đối tác) | Phía bên kia của một giao dịch: người bán đối với người mua, và người mua đối với người bán. | [1](01-order-book-basics.md) |
| Crossing the spread (vượt spread) | Lệnh đến có giá chạm tới giá tốt nhất của phía đối diện nên giao dịch được ngay. Lệnh như vậy gọi là *marketable* (có thể khớp ngay). | [1](01-order-book-basics.md) |
| Depth (độ sâu) | Tổng số lượng đang chờ ở một mức giá, hoặc qua nhiều mức giá. | [1](01-order-book-basics.md) |
| Exchange (sàn giao dịch) | Nơi người mua và người bán giao dịch với nhau. Lõi của nó là engine khớp lệnh. | [1](01-order-book-basics.md) |
| Execution report (báo cáo khớp lệnh) | Thông điệp cho nhà giao dịch biết điều gì đã xảy ra với lệnh của họ. Ở đây, vai trò này do các [sự kiện](07-events.md) của engine đảm nhận. | [7](07-events.md) |
| Fill / partial fill (khớp / khớp một phần) | Một lần thực hiện một phần hoặc toàn bộ số lượng của lệnh. Sau một lần khớp *một phần*, lệnh vẫn còn số lượng dư. | [1](01-order-book-basics.md) |
| FIX | *Financial Information eXchange*, một giao thức dạng văn bản phổ biến để nhà giao dịch gửi lệnh tới sàn. Gateway sẽ là nơi phân tích (parse) nó. | [2](02-architecture.md) |
| FOK (fill-or-kill) | Lệnh phải khớp đủ ngay lập tức, nếu không thì bị hủy hoàn toàn. Chưa được cài đặt ở đây (bài tập ở chương 11). | [1](01-order-book-basics.md) |
| Gateway (cổng nhận lệnh) | Phần của sàn nhận lệnh từ mạng, kiểm tra chúng, rồi chuyển cho engine khớp lệnh. | [2](02-architecture.md) |
| Instrument (mã giao dịch) | Thứ được giao dịch: một cổ phiếu, một hợp đồng tương lai, một cặp tiền tệ. Mỗi mã có một sổ lệnh riêng. | [1](01-order-book-basics.md) |
| IOC (immediate-or-cancel) | Lệnh khớp được bao nhiêu thì khớp ngay, phần dư bị hủy chứ không nằm chờ. Chưa được cài đặt ở đây (bài tập ở chương 11). | [1](01-order-book-basics.md) |
| Limit order (lệnh giới hạn) | Lệnh có một mức giá tệ nhất mà người đặt chấp nhận: "mua ở giá này hoặc thấp hơn", "bán ở giá này hoặc cao hơn". Đây là loại lệnh duy nhất engine này hỗ trợ. | [1](01-order-book-basics.md) |
| Liquidity (thanh khoản) | Có thể giao dịch được bao nhiêu mà không làm giá dịch chuyển nhiều. Lệnh đang chờ *cung cấp* thanh khoản; taker *lấy đi* thanh khoản. | [1](01-order-book-basics.md) |
| Maker | Lệnh đang chờ trong một giao dịch. Nó đã nằm trong sổ từ trước. | [1](01-order-book-basics.md) |
| Market data (dữ liệu thị trường) | Luồng dữ liệu công khai về các giao dịch và thay đổi của sổ lệnh mà sàn phát ra. | [2](02-architecture.md) |
| Matching engine (engine khớp lệnh) | Thành phần ghép lệnh mua với lệnh bán và quyết định giá giao dịch. Chính là toàn bộ repo này. | [1](01-order-book-basics.md) |
| Price level (mức giá) | Tất cả các lệnh đang chờ ở một mức giá trên một phía của sổ, được giữ thành một hàng đợi FIFO. | [1](01-order-book-basics.md) |
| Price-time priority (ưu tiên giá-thời gian) | Quy tắc khớp lệnh: giá tốt hơn được khớp trước; cùng giá thì lệnh đến sớm hơn được khớp trước. | [1](01-order-book-basics.md) |
| Resting order (lệnh đang chờ) | Lệnh nằm trong sổ vì khi đến, nó không thể giao dịch ngay. | [1](01-order-book-basics.md) |
| Self-trade prevention (chống tự giao dịch) | Quy tắc ngăn lệnh mua của một nhà giao dịch khớp với chính lệnh bán của họ. Chưa được cài đặt ở đây. | [1](01-order-book-basics.md) |
| Spread | Ask tốt nhất trừ bid tốt nhất. | [1](01-order-book-basics.md) |
| Sweep (quét qua nhiều mức giá) | Một taker giao dịch xuyên qua nhiều mức giá liên tiếp. | [6](06-matching-algorithm.md) |
| Taker | Lệnh vừa đến trong một giao dịch. Nó "lấy đi" thanh khoản mà một maker để lại trong sổ. | [1](01-order-book-basics.md) |
| Tick (bước giá) | Bước giá nhỏ nhất. Engine này chỉ làm việc với tick nguyên `0..=4095`; việc đổi sang giá thật là trách nhiệm của bên gọi. | [1](01-order-book-basics.md) |

## Cấu trúc dữ liệu và thuật toán

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Amortised cost (chi phí khấu hao) | Chi phí trung bình của mỗi thao tác trên một chuỗi dài, dù một vài thao tác riêng lẻ chậm hơn (ví dụ khi tệp WAL phải lớn thêm). | [9](09-write-ahead-log.md) |
| Arena | Một mảng lớn được cấp phát sẵn, chứa mọi đối tượng cùng loại. Các đối tượng được gọi bằng chỉ số thay vì con trỏ. Ở đây: order pool. | [3](03-order-pool.md) |
| Avalanche (hiệu ứng tuyết lở) | Tính chất của một hàm băm tốt: đổi một bit đầu vào thì khoảng một nửa số bit đầu ra bị đổi. | [5](05-hash-map.md) |
| Backward-shift deletion (xóa bằng cách dịch lùi) | Cách xóa khỏi bảng địa chỉ mở mà không cần tombstone: các phần tử đứng sau chỗ trống được dịch lùi để lấp chỗ đó, nếu việc đó vẫn giữ cho chúng tìm thấy được. | [5](05-hash-map.md) |
| Big-O (O(1), O(n)) | Khối lượng công việc tăng thế nào theo kích thước đầu vào. O(1) nghĩa là "công việc không đổi dù sổ lệnh lớn đến đâu"; O(n) nghĩa là "tỉ lệ với n". | [1](01-order-book-basics.md) |
| Bit-scan (quét bit) | Lệnh CPU tìm bit được đặt thấp nhất hoặc cao nhất trong một word. Rust cung cấp nó qua `trailing_zeros` / `leading_zeros`. | [4](04-price-ladder-bitset.md) |
| Bitset | Một mảng bit, mỗi phần tử một bit, ghi lại có/không cho từng phần tử. Ở đây: mỗi tick một bit, "có lệnh nào đang chờ ở mức giá này không?". | [4](04-price-ladder-bitset.md) |
| Deterministic (tất định) | Cùng đầu vào thì luôn cùng đầu ra. Không đồng hồ, số ngẫu nhiên hay thời điểm chạy của luồng nào có thể làm đổi kết quả. | [2](02-architecture.md) |
| FIFO | *First in, first out* (vào trước, ra trước): hàng đợi mà phần tử cũ nhất rời đi trước. Mỗi mức giá là một hàng đợi FIFO. | [3](03-order-pool.md) |
| Free list / free stack (danh sách / ngăn xếp ô trống) | Danh sách các ô của arena hiện không dùng. Ở đây nó là một ngăn xếp, nên ô vừa được giải phóng gần nhất sẽ được dùng lại trước. | [3](03-order-pool.md) |
| Hash function (hàm băm) | Hàm biến một khóa (một order id) thành một con số dùng để chọn ô trong bảng. | [5](05-hash-map.md) |
| Intrusive linked list (danh sách liên kết intrusive) | Danh sách liên kết có các liên kết `next` / `prev` nằm ngay bên trong các phần tử, nên không phải cấp phát nút riêng. | [3](03-order-pool.md) |
| Invariant (bất biến) | Điều phải đúng tại những thời điểm nhất định, ví dụ "bit của một tick được đặt khi và chỉ khi mức giá đó không rỗng". | [6](06-matching-algorithm.md) |
| Linear probing (dò tuyến tính) | Quy tắc xử lý va chạm của địa chỉ mở: nếu ô đã có người, thử ô kế tiếp, rồi ô kế nữa. | [5](05-hash-map.md) |
| Load factor (hệ số tải) | Bảng băm đầy đến đâu: số phần tử chia cho số ô. Việc dò chậm dần khi con số này tiến gần 1. | [5](05-hash-map.md) |
| Open addressing (địa chỉ mở) | Bảng băm lưu phần tử trực tiếp trong mảng các ô, thay vì trong một danh sách liên kết cho mỗi ô. | [5](05-hash-map.md) |
| Probe / probe chain (lần dò / chuỗi dò) | Việc kiểm tra một ô trong lúc tra cứu / chuỗi các ô mà một lần tra cứu phải kiểm tra. | [5](05-hash-map.md) |
| Seed | Một số ngẫu nhiên bí mật trộn vào hàm băm, để người ngoài không đoán được những id nào sẽ va chạm. | [5](05-hash-map.md) |
| Sentinel (giá trị đánh dấu đặc biệt) | Giá trị đặc biệt có nghĩa "ở đây không có gì", ví dụ `NIL = u32::MAX` cho "không có lệnh kế tiếp" hoặc `u64::MAX` cho "ô băm trống". | [3](03-order-pool.md) |
| SplitMix64 | Một hàm nhỏ, nhanh, trộn rất kỹ một số 64 bit. Được dùng làm hàm băm và trong checksum của WAL. | [5](05-hash-map.md) |
| Tombstone | Dấu để lại ở một ô băm đã bị xóa, để các lần tra cứu tiếp tục dò qua nó. Bảng băm này tránh được tombstone nhờ xóa bằng cách dịch lùi. | [5](05-hash-map.md) |

## Phần cứng và hiệu năng

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Alignment / padding (căn lề / byte đệm) | Phần lớn các kiểu dữ liệu phải bắt đầu ở địa chỉ là bội số của kích thước của chúng (*alignment*). Trình biên dịch chèn các byte *padding* không dùng đến để đảm bảo điều đó. | [3](03-order-pool.md) |
| Benchmark | Chương trình đo xem một đoạn code chạy nhanh thế nào. | [10](10-testing-and-performance.md) |
| Branch prediction (dự đoán nhánh) | CPU đoán trước một câu lệnh `if` sẽ rẽ về đâu để tiếp tục làm việc; đoán sai thì mất thời gian. Nhánh dễ đoán gần như miễn phí. | [6](06-matching-algorithm.md) |
| Cache / cache line | Bộ nhớ nhỏ và nhanh bên trong CPU. Dữ liệu di chuyển giữa bộ nhớ chính và cache theo từng khối cố định gọi là *cache line*, thường là 64 byte. | [3](03-order-pool.md) |
| False sharing | Hai luồng ghi vào hai biến khác nhau nhưng tình cờ nằm chung một cache line, khiến cache line đó bị chuyền qua lại giữa các nhân CPU. Khắc phục bằng padding (`CachePadded`). | [8](08-ring-buffer.md) |
| Heap allocation (cấp phát trên heap) | Xin bộ phận cấp phát bộ nhớ (`malloc`, `Box::new`, một `Vec` đang lớn lên) cấp bộ nhớ ngay lúc chạy. Chậm và khó đoán, nên hot path tránh nó. | [2](02-architecture.md) |
| Hot path | Đoạn code chạy cho mọi lệnh. Nó phải là phần nhanh nhất. | [2](02-architecture.md) |
| Latency / tail latency (độ trễ / độ trễ đuôi) | Thời gian để xử lý một yêu cầu / thời gian của những yêu cầu chậm nhất (ví dụ chậm nhất 1 trên 100). Sàn giao dịch quan tâm nhất đến phần đuôi. | [8](08-ring-buffer.md) |
| Prefetcher (bộ nạp trước) | Phần cứng CPU nhận ra bạn đang đọc bộ nhớ theo thứ tự và nạp sớm các cache line kế tiếp. Các mảng phẳng được hưởng lợi từ nó. | [3](03-order-pool.md) |
| Throughput (thông lượng) | Số yêu cầu được xử lý mỗi giây. | [10](10-testing-and-performance.md) |

## Rust

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Associated function (hàm liên kết) | Hàm nằm trong khối `impl` nhưng không nhận `self`, được gọi dưới dạng `Type::function(...)`. | [6](06-matching-algorithm.md) |
| Borrow checker | Phần của trình biên dịch thực thi quy tắc của Rust: tại một thời điểm, một giá trị chỉ có thể có hoặc một tham chiếu `&mut`, hoặc một số bất kỳ tham chiếu `&`. | [3](03-order-pool.md) |
| `dyn Trait` | Một giá trị được dùng thông qua một trait ngay lúc chạy, khi trình biên dịch không biết kiểu cụ thể của nó. Mỗi lời gọi đi qua một bảng nhỏ chứa các con trỏ hàm. | [6](06-matching-algorithm.md) |
| Generics / monomorphisation (kiểu tổng quát / đơn hình hóa) | `fn f<S: EventSink>(...)` dùng được cho mọi `S`. Trình biên dịch *đơn hình hóa* nó (monomorphize), tức tạo ra một bản sao của `f` cho từng kiểu cụ thể, nên các lời gọi là gọi trực tiếp và có thể được inline. | [6](06-matching-algorithm.md) |
| `#[inline]` | Gợi ý để chép thân của hàm vào các nơi gọi nó, bỏ đi chi phí của lời gọi hàm. | [6](06-matching-algorithm.md) |
| `MaybeUninit<T>` | Vùng nhớ có thể chưa chứa một `T` hợp lệ. Đọc nó như một `T` trước khi ghi là hành vi không xác định. | [8](08-ring-buffer.md) |
| Panic / `panic = "abort"` | Một panic dừng luồng hiện tại vì có bug (ví dụ một `assert!` thất bại). Với `panic = "abort"` trong profile của binary cuối cùng, nó kết thúc cả tiến trình. | [3](03-order-pool.md), [10](10-testing-and-performance.md) |
| `#[repr(C)]` | Sắp các trường của struct theo đúng thứ tự khai báo, như C. Không có nó, Rust có thể đổi thứ tự các trường để tiết kiệm chỗ. | [3](03-order-pool.md) |
| `Send` / `Sync` | Các marker trait. `Send`: một giá trị có thể được chuyển sang luồng khác. `Sync`: một giá trị có thể được *chia sẻ* (`&T`) giữa các luồng. | [8](08-ring-buffer.md) |
| `?Sized` | Cho phép một tham số kiểu generic là kiểu có kích thước không biết lúc biên dịch, chẳng hạn `dyn EventSink`. | [6](06-matching-algorithm.md) |
| Trait | Tập các phương thức mà một kiểu cam kết cung cấp, giống một interface. `EventSink` là một trait. | [7](07-events.md) |
| Undefined behaviour, UB (hành vi không xác định) | Vi phạm một quy tắc mà trình biên dịch dựa vào (ví dụ đọc bộ nhớ chưa khởi tạo). Khi đó chương trình có thể làm *bất cứ điều gì*; đây luôn là bug. | [8](08-ring-buffer.md) |
| `unsafe` | Khối mà lập trình viên, chứ không phải trình biên dịch, cam kết rằng các quy tắc được tuân thủ. Mọi khối `unsafe` ở đây đều có một comment `// SAFETY:` giải thích vì sao nó đúng đắn. | [8](08-ring-buffer.md) |
| `UnsafeCell<T>` | Cách hợp lệ duy nhất trong Rust để thay đổi dữ liệu nằm sau một tham chiếu chia sẻ `&`. Mọi kiểu interior-mutability an toàn đều được xây trên nó. | [8](08-ring-buffer.md) |

## Lập trình đồng thời

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Atomic (`AtomicUsize`, `AtomicBool`) | Biến mà nhiều luồng có thể đọc và ghi an toàn mà không cần khóa; mỗi thao tác diễn ra trọn vẹn hoặc không diễn ra. | [8](08-ring-buffer.md) |
| Compare-and-swap, CAS (so sánh rồi đổi) | Thao tác atomic "chỉ đổi giá trị thành B nếu nó vẫn đang là A", cần khi nhiều luồng cùng ghi một biến. Hàng đợi SPSC không cần đến nó. | [8](08-ring-buffer.md) |
| Data race (tranh chấp dữ liệu) | Hai luồng cùng lúc truy cập một vùng nhớ, ít nhất một luồng ghi, mà không có đồng bộ hóa. Trong Rust đây là hành vi không xác định. | [8](08-ring-buffer.md) |
| Disruptor (LMAX) | Thiết kế nổi tiếng của sàn LMAX: một bộ đệm vòng cấp phát sẵn nuôi một luồng bận rộn duy nhất, luồng này sở hữu toàn bộ trạng thái. Repo này đi theo tinh thần của nó. | [2](02-architecture.md) |
| Lock-free (không dùng khóa) | Các luồng phối hợp bằng atomic thay vì khóa (mutex), nên không luồng nào phải đợi luồng khác nhả khóa. | [8](08-ring-buffer.md) |
| Memory ordering (`Relaxed`, `Release`, `Acquire`) (thứ tự bộ nhớ) | Các quy tắc về việc khi nào những gì một luồng ghi trở nên nhìn thấy được với luồng khác. Một lệnh store `Release` ghép với một lệnh load `Acquire` đảm bảo mọi thứ được ghi trước store đều nhìn thấy được sau load. `Relaxed` không đảm bảo như vậy. | [8](08-ring-buffer.md) |
| Ring buffer (bộ đệm vòng) | Một mảng kích thước cố định dùng làm hàng đợi, trong đó vị trí đọc và vị trí ghi quay vòng về đầu mảng. | [8](08-ring-buffer.md) |
| Spin loop (vòng lặp chờ) | Chờ bằng cách kiểm tra đi kiểm tra lại một điều kiện thay vì ngủ. Phản ứng nhanh, nhưng giữ một nhân CPU luôn bận. | [8](08-ring-buffer.md) |
| SPSC | *Single producer, single consumer* (một producer, một consumer): hàng đợi có đúng một luồng ghi và một luồng đọc. Chính ràng buộc này làm nó đơn giản và nhanh. | [8](08-ring-buffer.md) |
| Thread (luồng) | Một dòng thực thi độc lập bên trong chương trình. Ở đây, gateway và engine khớp lệnh chạy trên các luồng riêng. | [2](02-architecture.md) |

## Tệp và độ bền vững

| Thuật ngữ | Ý nghĩa | Chương |
|---|---|---|
| Checksum (mã kiểm tra) | Một con số nhỏ được tính từ một dãy byte và lưu kèm theo chúng. Nếu các byte bị đổi, checksum (rất có thể) không còn khớp. | [9](09-write-ahead-log.md) |
| CRC / CRC32 | Một họ checksum chuẩn (dùng trong tệp zip và Ethernet). Repo này dùng checksum dựa trên SplitMix64 để khỏi phụ thuộc thư viện ngoài. | [9](09-write-ahead-log.md) |
| Dirty page | Một trang (page) của tệp đã bị sửa trong bộ nhớ nhưng chưa được ghi xuống đĩa. | [9](09-write-ahead-log.md) |
| Durability (độ bền vững) | Cam kết rằng dữ liệu vẫn còn sau khi máy sập hoặc mất điện, một khi nó đã được báo là đã lưu. | [9](09-write-ahead-log.md) |
| Endianness / little-endian (thứ tự byte) | Thứ tự byte của một số nhiều byte trong bộ nhớ. *Little-endian* lưu byte thấp nhất trước. WAL luôn dùng little-endian, nên tệp chạy được trên mọi máy. | [9](09-write-ahead-log.md) |
| `fsync` / `sync_all` / `msync` | Các lời gọi buộc hệ điều hành ghi dữ liệu đang nằm trong bộ nhớ đệm xuống đĩa. Cho đến khi một trong số chúng thành công, mất điện có thể làm mất dữ liệu. | [9](09-write-ahead-log.md) |
| Header / magic number | Những byte đầu tiên của một tệp, mô tả định dạng của nó. *Magic number* là một giá trị cố định dùng để nhận ra loại tệp. | [9](09-write-ahead-log.md) |
| MAC | *Message authentication code*: một checksum cần khóa bí mật, nên kẻ tấn công không thể giả mạo. Checksum của WAL **không phải** là MAC. | [9](09-write-ahead-log.md) |
| Memory-mapped file, mmap (tệp ánh xạ bộ nhớ) | Một tệp được làm cho trông như một mảng trong bộ nhớ. Ghi vào mảng là ghi vào tệp, thông qua page cache. | [9](09-write-ahead-log.md) |
| Page / page cache / page fault | Hệ điều hành quản lý bộ nhớ theo từng *page* (trang, thường 4 KiB). *Page cache* giữ các page của tệp được dùng gần đây trong RAM. *Page fault* xảy ra khi chương trình chạm vào một page chưa nằm trong RAM. | [9](09-write-ahead-log.md) |
| Replay (phát lại) | Dựng lại trạng thái bằng cách đưa các yêu cầu đã ghi log vào một engine mới tinh một lần nữa. Làm được vì engine là tất định. | [9](09-write-ahead-log.md) |
| `SIGBUS` | Tín hiệu Unix giết một tiến trình khi nó chạm vào page ánh xạ bộ nhớ mà hệ thống không thể đỡ bằng chỗ lưu thật (ví dụ đĩa đầy, hoặc tệp bị người khác thu nhỏ). | [9](09-write-ahead-log.md) |
| Sparse file (tệp thưa) | Tệp có những phần chưa ghi không chiếm chỗ trên đĩa; hệ điều hành đọc chúng ra toàn số 0. | [9](09-write-ahead-log.md) |
| Syscall (lời gọi hệ thống) | Yêu cầu từ chương trình gửi tới nhân (kernel) của hệ điều hành (mở tệp, ghi tệp...). Chậm hơn nhiều so với một lời gọi hàm thông thường. | [9](09-write-ahead-log.md) |
| Torn write / torn record (ghi dở) | Một lần ghi bị sự cố làm gián đoạn giữa chừng, để lại một bản ghi nửa mới nửa cũ (hoặc toàn số 0). Checksum phát hiện được tình huống này. | [9](09-write-ahead-log.md) |
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
| loom | Công cụ chạy một test đồng thời qua mọi cách đan xen có thể có của các luồng, để tìm bug về thứ tự. Được gợi ý làm bài tập. | [10](10-testing-and-performance.md) |
| LTO | *Link-time optimisation* (tối ưu hóa lúc liên kết): trình biên dịch tối ưu xuyên qua ranh giới giữa các crate khi build binary cuối cùng. | [10](10-testing-and-performance.md) |
| Miri | Trình thông dịch cho Rust, phát hiện hành vi không xác định trong code `unsafe`. CI chạy các unit test dưới nó. | [10](10-testing-and-performance.md) |
| MSRV | *Minimum supported Rust version* (phiên bản Rust tối thiểu được hỗ trợ): trình biên dịch cũ nhất mà crate cam kết build được (ở đây là 1.88). | [10](10-testing-and-performance.md) |
| Unit test | Test nhỏ nằm cạnh code mà nó kiểm thử (`#[cfg(test)] mod tests`), nên thấy được cả các phần private. | [10](10-testing-and-performance.md) |

**Quay lại:** [README](../../README.vi.md) · [1. Kiến thức cơ bản về sổ lệnh](01-order-book-basics.md)
