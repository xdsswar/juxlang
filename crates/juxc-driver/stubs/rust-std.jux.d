// juxc rust.std stub cache-version 28
// bindgen -- generated from 2 rustdoc JSON crate(s) (format_version 58)

package rust.std;

@rust("std::env::consts::ARCH")
public const String ARCH;

/** An error returned by [`LocalKey::try_with`](struct.LocalKey.html#method.try_with). */
@rust("std::thread::AccessError")
@RustClone
public class AccessError {
}

/** An error which can be returned when parsing an IP address or a socket address. */
@rust("std::net::AddrParseError")
@RustClone
public class AddrParseError implements Any, Clone, CloneToUninit, Debug, Display, Eq, Error, StructuralPartialEq {
}

/** The `AllocError` error indicates an allocation failure */
@rust("std::alloc::AllocError")
@RustClone
public class AllocError implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Error, StructuralPartialEq {
}

/** An iterator over [`Path`] and its ancestors. */
@rust("std::path::Ancestors")
@RustClone
public class Ancestors {
    @MutSelf public Path? next();
}

/** This enum represent one control message of variable type. */
@rust("std::os::unix::net::AncillaryData")
public enum AncillaryData {
    ScmRights(ScmRights), ScmCredentials(ScmCredentials)
}

/** The error type which is returned from parsing the type a control message. */
@rust("std::os::unix::net::AncillaryError")
public enum AncillaryError {
    Unknown
}

/** A thread-safe reference-counting pointer. 'Arc' stands for 'Atomically */
@rust("std::sync::Arc")
@RustClone
public class Arc<T, A> implements ToOwned, ToString {
    public Arc(T data);
    @RustDefault public Arc();
    public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T data);
    public static Pin<T> try_pin(T data) throws AllocError;
    public static T try_new(T data) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static U map<U>(Arc this, (T) -> U f);
    public static Self.TryType try_map<R>(Arc this, (T) -> R f);
    public static T new_in(T data, A alloc);
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static T new_cyclic_in<F>((Weak<T, A>) -> T data_fn, A alloc);
    public static Pin<T> pin_in(T data, A alloc);
    public static Pin<T> try_pin_in(T data, A alloc) throws AllocError;
    public static T try_new_in(T data, A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static T try_unwrap(Arc this) throws Arc;
    public static T? into_inner(Arc this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public static MaybeUninit<T>[] new_uninit_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] new_zeroed_slice_in(uint len, A alloc);
    public T[] into_array() throws Arc;
    public unsafe T assume_init();
    public static T clone_from_ref(&T value);
    public static T try_clone_from_ref(&T value) throws AllocError;
    public static T clone_from_ref_in(&T value, A alloc);
    public static T try_clone_from_ref_in(&T value, A alloc) throws AllocError;
    public static unsafe Arc from_raw(T* ptr);
    public static T* into_raw(Arc this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    @RustRefOut public static A allocator(&Arc this);
    public static (T*, A) into_raw_with_allocator(Arc this);
    public static T* as_ptr(&Arc this);
    public static unsafe Arc from_raw_in(T* ptr, A alloc);
    public static Weak<T, A> downgrade(&Arc this);
    public static uint weak_count(&Arc this);
    public static uint strong_count(&Arc this);
    public static unsafe void increment_strong_count_in(T* ptr, A alloc);
    public static unsafe void decrement_strong_count_in(T* ptr, A alloc);
    public static bool ptr_eq(&Arc this, &Arc other);
    @RustRefOut public static T make_mut(&mut Arc this);
    public static T unwrap_or_clone(Arc this);
    @RustRefOut public static T? get_mut(&mut Arc this);
    @RustRefOut public static unsafe T get_mut_unchecked(&mut Arc this);
    public static bool is_unique(&Arc this);
    public T downcast<T>() throws Arc;
    public unsafe T downcast_unchecked<T>();
}

/** An iterator over the arguments of a process, yielding a [`String`] value for */
@rust("std::env::Args")
public class Args {
    @MutSelf public String? next();
}

/** An iterator over the arguments of a process, yielding an [`OsString`] value */
@rust("std::env::ArgsOs")
public class ArgsOs {
    @MutSelf public OsString? next();
}

/** This structure represents a safely precompiled version of a format string */
@rust("std::fmt::Arguments")
@RustClone
public class Arguments implements Any, Clone, CloneToUninit, Copy, Debug, Display {
    public static Arguments from_str(&String s);
    @RustRefOut public String? as_str();
}

/** A windowed iterator over a slice in overlapping chunks (`N` elements at a */
@rust("std::slice::ArrayWindows")
@RustClone
public class ArrayWindows<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @MutSelf public T[]? next();
}

/** A trait to borrow the file descriptor from an underlying object. */
@rust("std::os::fd::AsFd")
public interface AsFd {
    @RustBorrowsSelf public BorrowedFd as_fd();
}

/** A trait to borrow the handle from an underlying object. */
@rust("std::os::windows::io::AsHandle")
public interface AsHandle {
    @RustBorrowsSelf public BorrowedHandle as_handle();
}

/** A trait to extract the raw file descriptor from an underlying object. */
@rust("std::os::fd::AsRawFd")
public interface AsRawFd {
    public RawFd as_raw_fd();
}

/** Extracts raw handles. */
@rust("std::os::windows::io::AsRawHandle")
public interface AsRawHandle {
    public RawHandle as_raw_handle();
}

/** Extracts raw sockets. */
@rust("std::os::windows::io::AsRawSocket")
public interface AsRawSocket {
    public RawSocket as_raw_socket();
}

/** A trait to borrow the socket from an underlying object. */
@rust("std::os::windows::io::AsSocket")
public interface AsSocket {
    @RustBorrowsSelf public BorrowedSocket as_socket();
}

/** One of the 128 Unicode characters from U+0000 through U+007F, */
@rust("std::ascii::Char")
@RustClone
public enum AsciiChar implements Any, Clone, CloneToUninit, Copy, Debug, Default, Display, Eq, Hash, Ord, Step, StructuralPartialEq, TrustedStep {
    Null = 0, StartOfHeading = 1, StartOfText = 2, EndOfText = 3, EndOfTransmission = 4, Enquiry = 5, Acknowledge = 6, Bell = 7, Backspace = 8, CharacterTabulation = 9, LineFeed = 10, LineTabulation = 11, FormFeed = 12, CarriageReturn = 13, ShiftOut = 14, ShiftIn = 15, DataLinkEscape = 16, DeviceControlOne = 17, DeviceControlTwo = 18, DeviceControlThree = 19, DeviceControlFour = 20, NegativeAcknowledge = 21, SynchronousIdle = 22, EndOfTransmissionBlock = 23, Cancel = 24, EndOfMedium = 25, Substitute = 26, Escape = 27, InformationSeparatorFour = 28, InformationSeparatorThree = 29, InformationSeparatorTwo = 30, InformationSeparatorOne = 31, Space = 32, ExclamationMark = 33, QuotationMark = 34, NumberSign = 35, DollarSign = 36, PercentSign = 37, Ampersand = 38, Apostrophe = 39, LeftParenthesis = 40, RightParenthesis = 41, Asterisk = 42, PlusSign = 43, Comma = 44, HyphenMinus = 45, FullStop = 46, Solidus = 47, Digit0 = 48, Digit1 = 49, Digit2 = 50, Digit3 = 51, Digit4 = 52, Digit5 = 53, Digit6 = 54, Digit7 = 55, Digit8 = 56, Digit9 = 57, Colon = 58, Semicolon = 59, LessThanSign = 60, EqualsSign = 61, GreaterThanSign = 62, QuestionMark = 63, CommercialAt = 64, CapitalA = 65, CapitalB = 66, CapitalC = 67, CapitalD = 68, CapitalE = 69, CapitalF = 70, CapitalG = 71, CapitalH = 72, CapitalI = 73, CapitalJ = 74, CapitalK = 75, CapitalL = 76, CapitalM = 77, CapitalN = 78, CapitalO = 79, CapitalP = 80, CapitalQ = 81, CapitalR = 82, CapitalS = 83, CapitalT = 84, CapitalU = 85, CapitalV = 86, CapitalW = 87, CapitalX = 88, CapitalY = 89, CapitalZ = 90, LeftSquareBracket = 91, ReverseSolidus = 92, RightSquareBracket = 93, CircumflexAccent = 94, LowLine = 95, GraveAccent = 96, SmallA = 97, SmallB = 98, SmallC = 99, SmallD = 100, SmallE = 101, SmallF = 102, SmallG = 103, SmallH = 104, SmallI = 105, SmallJ = 106, SmallK = 107, SmallL = 108, SmallM = 109, SmallN = 110, SmallO = 111, SmallP = 112, SmallQ = 113, SmallR = 114, SmallS = 115, SmallT = 116, SmallU = 117, SmallV = 118, SmallW = 119, SmallX = 120, SmallY = 121, SmallZ = 122, LeftCurlyBracket = 123, VerticalLine = 124, RightCurlyBracket = 125, Tilde = 126, Delete = 127;

    public static AsciiChar? from_u8(ubyte b);
    public static unsafe AsciiChar from_u8_unchecked(ubyte b);
    public static AsciiChar? digit(ubyte d);
    public static unsafe AsciiChar digit_unchecked(ubyte d);
    public ubyte to_u8();
    public char to_char();
    @RustRefOut public String as_str();
    public AsciiChar to_uppercase();
    public AsciiChar to_lowercase();
    public bool eq_ignore_case(AsciiChar other);
    @MutSelf public void make_uppercase();
    @MutSelf public void make_lowercase();
    public bool is_alphabetic();
    public bool is_uppercase();
    public bool is_lowercase();
    public bool is_alphanumeric();
    public bool is_digit();
    public bool is_octdigit();
    public bool is_hexdigit();
    public bool is_punctuation();
    public bool is_graphic();
    public bool is_whitespace();
    public bool is_control();
    public EscapeDefault escape_ascii();
}

/** Extension methods for ASCII-subset only operations. */
@rust("std::ascii::AsciiExt")
public interface AsciiExt {
    public bool is_ascii();
    public Self.Owned to_ascii_uppercase();
    public Self.Owned to_ascii_lowercase();
    public bool eq_ignore_ascii_case(&Self other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
}

/** A simple wrapper around a type to assert that it is unwind safe. */
@rust("std::panic::AssertUnwindSafe")
public class AssertUnwindSafe<T> implements Any, AsyncIterator, Debug, Default, Deref, DerefMut, Future, IntoAsyncIterator, IntoFuture, Receiver, RefUnwindSafe, UnwindSafe {
    @RustDefault public AssertUnwindSafe();
}

/** An ordered map based on a [B-Tree]. */
@rust("std::collections::BTreeMap")
@RustIndexRef
@RustClone
@RustCollection
public class BTreeMap<K, V, A> implements ToOwned {
    public BTreeMap();
    @MutSelf public void clear();
    public static Map<K, V> new_in(A alloc);
    @RustRefOut public V? get<Q>(&Q key);
    public (K, V)? get_key_value<Q>(&Q k);
    public (K, V)? first_key_value();
    @MutSelf @RustBorrowsSelf public OccupiedEntry<K, V, A>? first_entry();
    @MutSelf public (K, V)? pop_first();
    public (K, V)? last_key_value();
    @MutSelf @RustBorrowsSelf public OccupiedEntry<K, V, A>? last_entry();
    @MutSelf public (K, V)? pop_last();
    public bool contains_key<Q>(&Q key);
    @MutSelf @RustRefOut public V? get_mut<Q>(&Q key);
    @MutSelf public V? insert(K key, V value);
    @MutSelf @RustBorrowsSelf @RustRefOut public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf public V? remove<Q>(&Q key);
    @MutSelf public (K, V)? remove_entry<Q>(&Q key);
    @MutSelf public void retain<F>((K, V) -> bool f);
    @MutSelf public void append(&mut BTreeMap other);
    @MutSelf public void merge(BTreeMap other, (K, V, V) -> V conflict);
    @RustBorrowsSelf public Range<K, V> range<T, R>(R range);
    @MutSelf @RustBorrowsSelf public RangeMut<K, V> range_mut<T, R>(R range);
    @MutSelf @RustBorrowsSelf public Entry<K, V, A> entry(K key);
    @MutSelf public BTreeMap split_off<Q>(&Q key);
    @MutSelf @RustBorrowsSelf public ExtractIf<K, V, R, F, A> extract_if<F, R>(R range, (K, V) -> bool pred);
    public IntoKeys<K, V, A> into_keys();
    public IntoValues<K, V, A> into_values();
    @RustBorrowsSelf public Iter<K, V> iter();
    @MutSelf @RustBorrowsSelf public IterMut<K, V> iter_mut();
    @RustBorrowsSelf public Keys<K, V> keys();
    @RustBorrowsSelf public Values<K, V> values();
    @MutSelf @RustBorrowsSelf public ValuesMut<K, V> values_mut();
    public uint len();
    public bool is_empty();
    @RustBorrowsSelf public Cursor<K, V> lower_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf public CursorMut<K, V, A> lower_bound_mut<Q>(Bound<Q> bound);
    @RustBorrowsSelf public Cursor<K, V> upper_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf public CursorMut<K, V, A> upper_bound_mut<Q>(Bound<Q> bound);
}

/** An ordered set based on a B-Tree. */
@rust("std::collections::BTreeSet")
@RustClone
@RustCollection
public class BTreeSet<T, A> implements ToOwned {
    public BTreeSet();
    public static Set<T> new_in(A alloc);
    @RustBorrowsSelf public Range<T> range<K, R>(R range);
    @RustBorrowsSelf public Difference<T, A> difference(&Set<T> other);
    @RustBorrowsSelf public SymmetricDifference<T> symmetric_difference(&Set<T> other);
    @RustBorrowsSelf public Intersection<T, A> intersection(&Set<T> other);
    @RustBorrowsSelf public Union<T> union(&Set<T> other);
    @MutSelf public void clear();
    public bool contains<Q>(&Q value);
    @RustRefOut public T? get<Q>(&Q value);
    public bool is_disjoint(&Set<T> other);
    public bool is_subset(&Set<T> other);
    public bool is_superset(&Set<T> other);
    @RustRefOut public T? first();
    @RustRefOut public T? last();
    @MutSelf public T? pop_first();
    @MutSelf public T? pop_last();
    @MutSelf public bool insert(T value);
    @MutSelf public T? replace(T value);
    @MutSelf @RustRefOut public T get_or_insert(T value);
    @MutSelf @RustRefOut public T get_or_insert_with<Q, F>(&Q value, (Q) -> T f);
    @MutSelf @RustBorrowsSelf public Entry<T, A> entry(T value);
    @MutSelf public bool remove<Q>(&Q value);
    @MutSelf public T? take<Q>(&Q value);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void append(&mut BTreeSet other);
    @MutSelf public BTreeSet split_off<Q>(&Q value);
    @MutSelf @RustBorrowsSelf public ExtractIf<T, R, F, A> extract_if<F, R>(R range, (T) -> bool pred);
    @RustBorrowsSelf public Iter<T> iter();
    public uint len();
    public bool is_empty();
    @RustBorrowsSelf public Cursor<T> lower_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf public CursorMut<T, A> lower_bound_mut<Q>(Bound<Q> bound);
    @RustBorrowsSelf public Cursor<T> upper_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf public CursorMut<T, A> upper_bound_mut<Q>(Bound<Q> bound);
}

/** A captured OS thread stack backtrace. */
@rust("std::backtrace::Backtrace")
public class Backtrace {
    public static Backtrace capture();
    public static Backtrace force_capture();
    public static Backtrace disabled();
    public BacktraceStatus status();
    @RustRefOut public BacktraceFrame[] frames();
}

/** A single frame of a backtrace. */
@rust("std::backtrace::BacktraceFrame")
public class BacktraceFrame {
}

/** The current status of a backtrace, indicating whether it was captured or */
@rust("std::backtrace::BacktraceStatus")
public enum BacktraceStatus {
    Unsupported, Disabled, Captured
}

/** The configuration for whether and how the default panic hook will capture */
@rust("std::panic::BacktraceStyle")
@RustClone
public enum BacktraceStyle {
    Short, Full, Off
}

/** A barrier enables multiple threads to synchronize the beginning */
@rust("std::sync::Barrier")
public class Barrier {
    public Barrier(uint n);
    public BarrierWaitResult wait();
}

/** A `BarrierWaitResult` is returned by [`Barrier::wait()`] when all threads */
@rust("std::sync::BarrierWaitResult")
public class BarrierWaitResult {
    public bool is_leader();
}

/** A priority queue implemented with a binary heap. */
@rust("std::collections::BinaryHeap")
@RustClone
@RustCollection
public class BinaryHeap<T, A> implements ToOwned {
    public BinaryHeap();
    public static BinaryHeap<T> with_capacity(uint capacity);
    public static BinaryHeap<T, A> new_in(A alloc);
    public static BinaryHeap<T, A> with_capacity_in(uint capacity, A alloc);
    public static unsafe BinaryHeap<T, A> from_raw_vec(Vec<T> vec);
    @MutSelf @RustBorrowsSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public T? pop();
    @MutSelf public T? pop_if((T) -> bool predicate);
    @MutSelf public void push(T item);
    public Vec<T> into_sorted_vec();
    @MutSelf public void append(&mut BinaryHeap other);
    @MutSelf @RustBorrowsSelf public DrainSorted<T, A> drain_sorted();
    @MutSelf public void retain<F>((T) -> bool f);
    @RustBorrowsSelf public Iter<T> iter();
    public IntoIterSorted<T, A> into_iter_sorted();
    @RustRefOut public T? peek();
    public uint capacity();
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public unsafe T[] as_mut_slice();
    public Vec<T> into_vec();
    @RustRefOut public A allocator();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain();
    @MutSelf public void clear();
}

/** A borrowed buffer of initially uninitialized elements, which is incrementally filled. */
@rust("std::io::BorrowedBuf")
public class BorrowedBuf<T> implements Any, Debug {
    public uint capacity();
    public uint len();
    public bool is_init();
    @RustRefOut public T[] filled();
    @MutSelf @RustRefOut public T[] filled_mut();
    @RustRefOut public T[] into_filled();
    @RustRefOut public T[] into_filled_mut();
    @MutSelf @RustBorrowsSelf public BorrowedCursor<T> unfilled();
    @MutSelf @RustRefOut public BorrowedBuf clear();
    @MutSelf @RustRefOut public unsafe BorrowedBuf set_init();
}

/** A writeable view of the unfilled portion of a [`BorrowedBuf`]. */
@rust("std::io::BorrowedCursor")
public class BorrowedCursor<T> implements Any, Debug {
    @MutSelf @RustBorrowsSelf public BorrowedCursor<T> reborrow();
    public uint capacity();
    public uint written();
    public bool is_init();
    @MutSelf public unsafe void set_init();
    @MutSelf @RustRefOut public unsafe MaybeUninit<T>[] as_mut();
    @MutSelf @RustRefOut public BorrowedCursor advance_checked(uint n);
    @MutSelf @RustRefOut public unsafe BorrowedCursor advance(uint n);
    @MutSelf public void append(T[] buf);
    @MutSelf public R with_unfilled_buf<R>((BorrowedBuf<T>) -> R f);
    @MutSelf @RustRefOut public ubyte[] ensure_init();
}

/** A borrowed file descriptor. */
@rust("std::os::fd::BorrowedFd")
@RustClone
public class BorrowedFd implements AsFd, AsRawFd, IsTerminal {
    public static unsafe BorrowedFd borrow_raw(RawFd fd);
    public OwnedFd try_clone_to_owned() throws Error;
}

/** A borrowed handle. */
@rust("std::os::windows::io::BorrowedHandle")
@RustClone
public class BorrowedHandle implements AsHandle, AsRawHandle, IsTerminal {
    public static unsafe BorrowedHandle borrow_raw(RawHandle handle);
    public OwnedHandle try_clone_to_owned() throws Error;
}

/** A borrowed socket. */
@rust("std::os::windows::io::BorrowedSocket")
@RustClone
public class BorrowedSocket implements AsRawSocket, AsSocket {
    public static unsafe BorrowedSocket borrow_raw(RawSocket socket);
    public OwnedSocket try_clone_to_owned() throws Error;
}

/** An endpoint of a range of keys. */
@rust("std::ops::Bound")
@RustClone
public enum Bound<T> implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, StructuralPartialEq {
    Included(T), Excluded(T), Unbounded;

    @RustRefOut public Bound<T> as_ref();
    @MutSelf @RustRefOut public Bound<T> as_mut();
    public Bound<U> map<U, F>((T) -> U f);
    public Bound<T> copied();
    public Bound<T> cloned();
}

/** A pointer type that uniquely owns a heap allocation of type `T`. */
@rust("std::boxed::Box")
@RustClone
public class Box<T, A> implements ToOwned, ToString {
    public Box(T x);
    @RustDefault public Box();
    public T downcast<T>() throws Box;
    public unsafe T downcast_unchecked<T>();
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T x);
    public static Box try_new(T x) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static U map<U>(Box this, (T) -> U f);
    public static Self.TryType try_map<R>(Box this, (T) -> R f);
    public static Box new_in(T x, A alloc);
    public static Box try_new_in(T x, A alloc) throws AllocError;
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static Pin<Box> pin_in(T x, A alloc);
    public static T[] into_boxed_slice(Box boxed);
    public static T into_inner(Box boxed);
    public static (T, MaybeUninit<T>) take(Box boxed);
    public static T clone_from_ref(&T src);
    public static T try_clone_from_ref(&T src) throws AllocError;
    public static T clone_from_ref_in(&T src, A alloc);
    public static T try_clone_from_ref_in(&T src, A alloc) throws AllocError;
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public static MaybeUninit<T>[] try_new_uninit_slice(uint len) throws AllocError;
    public static MaybeUninit<T>[] try_new_zeroed_slice(uint len) throws AllocError;
    public static MaybeUninit<T>[] new_uninit_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] new_zeroed_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] try_new_uninit_slice_in(uint len, A alloc) throws AllocError;
    public static MaybeUninit<T>[] try_new_zeroed_slice_in(uint len, A alloc) throws AllocError;
    public T[] into_array() throws Box;
    public unsafe T assume_init();
    public static T write(Box boxed, T value);
    public static unsafe Box from_raw(T* raw);
    public static unsafe Box from_non_null(NonNull<T> ptr);
    public static T* into_raw(Box b);
    public static NonNull<T> into_non_null(Box b);
    public static unsafe Box from_raw_in(T* raw, A alloc);
    public static unsafe Box from_non_null_in(NonNull<T> raw, A alloc);
    public static (T*, A) into_raw_with_allocator(Box b);
    public static (NonNull<T>, A) into_non_null_with_allocator(Box b);
    public static T* as_mut_ptr(&mut Box b);
    public static T* as_ptr(&Box b);
    @RustRefOut public static A allocator(&Box b);
    @RustRefOut public static T leak(Box b);
    public static Pin<Box> into_pin(Box boxed);
    @MutSelf public I.Item? next();
}

/** A `BufRead` is a type of `Read`er which has an internal buffer, allowing it */
@rust("std::io::BufRead")
public interface BufRead {
    @MutSelf @RustRefOut public ubyte[] fill_buf() throws Error;
    @MutSelf public void consume(uint amount);
    @MutSelf public bool has_data_left() throws Error;
    @MutSelf public uint read_until(ubyte byte, &mut Vec<ubyte> buf) throws Error;
    @MutSelf public uint skip_until(ubyte byte) throws Error;
    @MutSelf public uint read_line(&mut String buf) throws Error;
    public Split<Self> split(ubyte byte);
    public Lines<Self> lines();
}

/** The `BufReader<R>` struct adds buffering to any reader. */
@rust("std::io::BufReader")
public class BufReader<R> implements BufRead, Read, Seek {
    public BufReader(R inner);
    public static BufReader<R> with_capacity(uint capacity, R inner);
    @MutSelf @RustRefOut public ubyte[] peek(uint n) throws Error;
    @RustRefOut public R get_ref();
    @MutSelf @RustRefOut public R get_mut();
    @RustRefOut public ubyte[] buffer();
    public uint capacity();
    public R into_inner();
    @MutSelf public void seek_relative(long offset) throws Error;
}

/** Wraps a writer and buffers its output. */
@rust("std::io::BufWriter")
public class BufWriter<W> implements Seek, Write {
    public BufWriter(W inner);
    public static BufWriter<W> with_capacity(uint capacity, W inner);
    public W into_inner() throws IntoInnerError<BufWriter<W>>;
    public (W, Result<Vec<ubyte>, WriterPanicked>) into_parts();
    @RustRefOut public W get_ref();
    @MutSelf @RustRefOut public W get_mut();
    @RustRefOut public ubyte[] buffer();
    public uint capacity();
}

/** Thread factory, which can be used in order to configure the properties of */
@rust("std::thread::Builder")
public class Builder {
    public Builder();
    public Builder name(String name);
    public Builder stack_size(uint size);
    public Builder no_hooks();
    public JoinHandle<T> spawn<F, T>(() -> T f) throws Error;
    public unsafe JoinHandle<T> spawn_unchecked<F, T>(() -> T f) throws Error;
    @RustBorrowsSelf public ScopedJoinHandle<T> spawn_scoped<F, T>(&Scope scope, () -> T f) throws Error;
}

/** A wrapper for `&[u8]` representing a human-readable string that's conventionally, but not */
@rust("std::bstr::ByteStr")
public class ByteStr implements Any, CloneToUninit, Debug, Deref, DerefMut, DerefPure, Display, Eq, Hash, Ord, Receiver {
    public ByteStr(&B bytes);
    @RustDefault public ByteStr();
    @RustRefOut public ByteStr as_byte_str();
    @MutSelf @RustRefOut public ByteStr as_mut_byte_str();
    @MutSelf public void sort_floats();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
    public bool is_ascii();
    @RustRefOut public Char[]? as_ascii();
    @RustRefOut public unsafe Char[] as_ascii_unchecked();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    public uint len();
    public bool is_empty();
    @RustRefOut public T? first();
    @MutSelf @RustRefOut public T? first_mut();
    public (T, T[])? split_first();
    @MutSelf public (T, T[])? split_first_mut();
    public (T, T[])? split_last();
    @MutSelf public (T, T[])? split_last_mut();
    @RustRefOut public T? last();
    @MutSelf @RustRefOut public T? last_mut();
    @RustRefOut public T[]? first_chunk();
    @MutSelf @RustRefOut public T[]? first_chunk_mut();
    public (T[], T[])? split_first_chunk();
    @MutSelf public (T[], T[])? split_first_chunk_mut();
    public (T[], T[])? split_last_chunk();
    @MutSelf public (T[], T[])? split_last_chunk_mut();
    @RustRefOut public T[]? last_chunk();
    @MutSelf @RustRefOut public T[]? last_chunk_mut();
    @RustRefOut public I.Output? get<I>(I index);
    @MutSelf @RustRefOut public I.Output? get_mut<I>(I index);
    @RustRefOut public unsafe I.Output get_unchecked<I>(I index);
    @MutSelf @RustRefOut public unsafe I.Output get_unchecked_mut<I>(I index);
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    public Range<T*> as_ptr_range();
    @MutSelf public Range<T*> as_mut_ptr_range();
    @RustRefOut public T[]? as_array();
    @MutSelf @RustRefOut public T[]? as_mut_array();
    @MutSelf public void swap(uint a, uint b);
    @MutSelf public unsafe void swap_unchecked(uint a, uint b);
    @MutSelf public void reverse();
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    @RustBorrowsSelf public Windows<T> windows(uint size);
    @RustBorrowsSelf public Chunks<T> chunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksMut<T> chunks_mut(uint chunk_size);
    @RustBorrowsSelf public ChunksExact<T> chunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksExactMut<T> chunks_exact_mut(uint chunk_size);
    @RustRefOut public unsafe T[][] as_chunks_unchecked();
    public (T[][], T[]) as_chunks();
    public (T[], T[][]) as_rchunks();
    @MutSelf @RustRefOut public unsafe T[][] as_chunks_unchecked_mut();
    @MutSelf public (T[][], T[]) as_chunks_mut();
    @MutSelf public (T[], T[][]) as_rchunks_mut();
    @RustBorrowsSelf public ArrayWindows<T> array_windows();
    @RustBorrowsSelf public RChunks<T> rchunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksMut<T> rchunks_mut(uint chunk_size);
    @RustBorrowsSelf public RChunksExact<T> rchunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksExactMut<T> rchunks_exact_mut(uint chunk_size);
    @RustBorrowsSelf public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    public (T[], T[])? split_once<F>((T) -> bool pred);
    public (T[], T[])? rsplit_once<F>((T) -> bool pred);
    public bool contains(&T x);
    public bool starts_with(T[] needle);
    public bool ends_with(T[] needle);
    @RustRefOut public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut public T[]? strip_suffix<P>(&P suffix);
    @RustRefOut public T[]? strip_circumfix<S, P>(&P prefix, &S suffix);
    @RustRefOut public T[] trim_prefix<P>(&P prefix);
    @RustRefOut public T[] trim_suffix<P>(&P suffix);
    public uint binary_search(&T x) throws Error;
    public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf public void sort_unstable();
    @MutSelf public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf public void partial_sort_unstable<R>(R range);
    @MutSelf public void partial_sort_unstable_by<F, R>(R range, (T, T) -> Ordering compare);
    @MutSelf public void partial_sort_unstable_by_key<K, F, R>(R range, (T) -> K f);
    @MutSelf public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public (T[], T[]) partition_dedup();
    @MutSelf public (T[], T[]) partition_dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public (T[], T[]) partition_dedup_by_key<K, F>((T) -> K key);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf public T[] shift_left(T[] inserted);
    @MutSelf public T[] shift_right(T[] inserted);
    @MutSelf public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf public void clone_from_slice(T[] src);
    @MutSelf public void copy_from_slice(T[] src);
    @MutSelf public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    public (T[], Simd<T>[], T[]) as_simd();
    @MutSelf public (T[], Simd<T>[], T[]) as_simd_mut();
    public bool is_sorted();
    public bool is_sorted_by<F>((T, T) -> bool compare);
    public bool is_sorted_by_key<F, K>((T) -> K f);
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off<R>(R range);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public Range<uint>? subslice_range(T[] subslice);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public T[] as_mut_slice();
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
}

/** A wrapper for `Vec<u8>` representing a human-readable string that's conventionally, but not */
@rust("std::bstr::ByteString")
@RustClone
@RustCollection
public class ByteString implements ToOwned, ToString {
    @RustDefault public ByteString();
    @MutSelf @RustBorrowsSelf public Splice<I.IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf @RustBorrowsSelf public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
    public (T*, uint, uint) into_raw_parts();
    public (NonNull<T>, uint, uint) into_parts();
    @RustRefOut public T[] const_make_global();
    public (T*, uint, uint, A) into_raw_parts_with_alloc();
    public (NonNull<T>, uint, uint, A) into_parts_with_alloc();
    public uint capacity();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf public void try_shrink_to_fit() throws TryReserveError;
    @MutSelf public void try_shrink_to(uint min_capacity) throws TryReserveError;
    public T[] into_boxed_slice();
    public T[] into_array() throws Self;
    @MutSelf public void truncate(uint len);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public T[] as_mut_slice();
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    @MutSelf public NonNull<T> as_non_null();
    @RustRefOut public A allocator();
    @MutSelf public unsafe void set_len(uint new_len);
    @MutSelf public T swap_remove(uint index);
    @MutSelf public void insert(uint index, T element);
    @MutSelf @RustRefOut public T insert_mut(uint index, T element);
    @MutSelf public T remove(uint index);
    @MutSelf public T? try_remove(uint index);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void retain_mut<F>((T) -> bool f);
    @MutSelf public void dedup_by_key<F, K>((T) -> K key);
    @MutSelf public void dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf @RustRefOut public T push_within_capacity(T value) throws T;
    @MutSelf public T? pop();
    @MutSelf public T? pop_if((T) -> bool predicate);
    @MutSelf @RustBorrowsSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public void append(&mut Self other);
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public void clear();
    public uint len();
    public bool is_empty();
    @MutSelf public Self split_off(uint at);
    @MutSelf public void resize_with<F>(uint new_len, () -> T f);
    @RustRefOut public T[] leak();
    @MutSelf @RustRefOut public MaybeUninit<T>[] spare_capacity_mut();
    @MutSelf public (T[], MaybeUninit<T>[]) split_at_spare_mut();
    public Vec<T[]> into_chunks();
    public Vec<U> recycle<U>();
    @MutSelf public void dedup();
    @MutSelf public void push(T value);
    @MutSelf @RustRefOut public T push_mut(T value);
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_slice(T[] other);
    @MutSelf public void extend_from_within<R>(R src);
    public Vec<T> into_flattened();
}

/** An iterator over `u8` values of a reader. */
@rust("std::io::Bytes")
public class Bytes<R> {
    @MutSelf public ubyte? next() throws Error;
}

/** A dynamically-sized view of a C string. */
@rust("std::ffi::CStr")
public class CStr implements Any, CloneToUninit, Debug, Eq, Hash, Ord, StructuralPartialEq {
    @RustDefault public CStr();
    @RustRefOut public static unsafe CStr from_ptr(c_char* ptr);
    @RustRefOut public static CStr from_bytes_until_nul(ubyte[] bytes) throws FromBytesUntilNulError;
    @RustRefOut public static CStr from_bytes_with_nul(ubyte[] bytes) throws FromBytesWithNulError;
    @RustRefOut public static unsafe CStr from_bytes_with_nul_unchecked(ubyte[] bytes);
    public c_char* as_ptr();
    public uint count_bytes();
    public bool is_empty();
    @RustRefOut public ubyte[] to_bytes();
    @RustRefOut public ubyte[] to_bytes_with_nul();
    @RustBorrowsSelf public Bytes bytes();
    @RustRefOut public String to_str() throws Utf8Error;
    public Display display();
    @RustRefOut public CStr as_c_str();
}

/** A type representing an owned, C-compatible, nul-terminated string with no nul bytes in the */
@rust("std::ffi::c_str::CString")
@RustClone
public class CString implements ToOwned {
    public CString(T t) throws NulError;
    @RustDefault public CString();
    public static unsafe CString from_vec_unchecked(Vec<ubyte> v);
    public static unsafe CString from_raw(c_char* ptr);
    public c_char* into_raw();
    public String into_string() throws IntoStringError;
    public Vec<ubyte> into_bytes();
    public Vec<ubyte> into_bytes_with_nul();
    @RustRefOut public ubyte[] as_bytes();
    @RustRefOut public ubyte[] as_bytes_with_nul();
    @RustRefOut public CStr as_c_str();
    public CStr into_boxed_c_str();
    public static unsafe CString from_vec_with_nul_unchecked(Vec<ubyte> v);
    public static CString from_vec_with_nul(Vec<ubyte> v) throws FromVecWithNulError;
    public c_char* as_ptr();
    public uint count_bytes();
    public bool is_empty();
    @RustRefOut public ubyte[] to_bytes();
    @RustRefOut public ubyte[] to_bytes_with_nul();
    @RustBorrowsSelf public Bytes bytes();
    @RustRefOut public String to_str() throws Utf8Error;
    public Display display();
}

/** Compile-time type information about `char`. */
@rust("std::mem::type_info::Char")
public class Char implements Any, Debug {
}

/** An iterator over the [`char`]s of a string slice, and their positions. */
@rust("std::str::CharIndices")
@RustClone
public class CharIndices implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String as_str();
    public uint offset();
    @MutSelf public (uint, char)? next();
}

/** An iterator over the [`char`]s of a string slice. */
@rust("std::str::Chars")
@RustClone
public class Chars implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String as_str();
    @MutSelf public char? next();
}

/** Representation of a running or exited child process. */
@rust("std::process::Child")
public class Child implements AsHandle, AsRawHandle, ChildExt, IntoRawHandle {
    public ChildStdin? stdin;
    public ChildStdout? stdout;
    public ChildStderr? stderr;
    @MutSelf public void kill() throws Error;
    public u32 id();
    @MutSelf public ExitStatus wait() throws Error;
    @MutSelf public ExitStatus? try_wait() throws Error;
    public Output wait_with_output() throws Error;
}

/** Os-specific extensions for [`Child`] */
@rust("std::os::linux::process::ChildExt")
public interface ChildExt {
    @RustRefOut public PidFd pidfd() throws Error;
    public PidFd into_pidfd() throws Self;
}

/** A handle to a child process's stderr. */
@rust("std::process::ChildStderr")
public class ChildStderr implements AsFd, AsHandle, AsRawFd, AsRawHandle, IntoRawFd, IntoRawHandle, Read {
}

/** A handle to a child process's standard input (stdin). */
@rust("std::process::ChildStdin")
public class ChildStdin implements AsFd, AsHandle, AsRawFd, AsRawHandle, IntoRawFd, IntoRawHandle, Write {
}

/** A handle to a child process's standard output (stdout). */
@rust("std::process::ChildStdout")
public class ChildStdout implements AsFd, AsHandle, AsRawFd, AsRawHandle, IntoRawFd, IntoRawHandle, Read {
}

/** An iterator over slice in (non-overlapping) chunks separated by a predicate. */
@rust("std::slice::ChunkBy")
@RustClone
public class ChunkBy<T, P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator over slice in (non-overlapping) mutable chunks separated */
@rust("std::slice::ChunkByMut")
public class ChunkByMut<T, P> implements Any, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::Chunks")
@RustClone
public class Chunks<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::ChunksExact")
@RustClone
public class ChunksExact<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @RustRefOut public T[] remainder();
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::ChunksExactMut")
public class ChunksExactMut<T> implements Any, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, Send, Sync, TrustedLen {
    @RustRefOut public T[] into_remainder();
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::ChunksMut")
public class ChunksMut<T> implements Any, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, Send, Sync, TrustedLen {
    @MutSelf public T[]? next();
}

/** A process builder, providing fine-grained control */
@rust("std::process::Command")
public class Command implements CommandExt {
    public Command(S program);
    @MutSelf @RustRefOut public Command arg<S>(S arg);
    @MutSelf @RustRefOut public Command args<I, S>(I args);
    @MutSelf @RustRefOut public Command env<K, V>(K key, V val);
    @MutSelf @RustRefOut public Command envs<I, K, V>(I vars);
    @MutSelf @RustRefOut public Command env_remove<K>(K key);
    @MutSelf @RustRefOut public Command env_clear();
    @MutSelf @RustRefOut public Command current_dir<P>(P dir);
    @MutSelf @RustRefOut public Command stdin<T>(T cfg);
    @MutSelf @RustRefOut public Command stdout<T>(T cfg);
    @MutSelf @RustRefOut public Command stderr<T>(T cfg);
    @MutSelf public Child spawn() throws Error;
    @MutSelf public Output output() throws Error;
    @MutSelf public ExitStatus status() throws Error;
    @RustRefOut public OsStr get_program();
    @RustBorrowsSelf public CommandArgs get_args();
    @RustBorrowsSelf public CommandEnvs get_envs();
    public CommandResolvedEnvs get_resolved_envs();
    @RustRefOut public Path? get_current_dir();
    public bool get_env_clear();
}

/** An iterator over the command arguments. */
@rust("std::process::CommandArgs")
public class CommandArgs {
    @MutSelf public OsStr? next();
}

/** An iterator over the command environment variables. */
@rust("std::process::CommandEnvs")
public class CommandEnvs {
    @MutSelf public (OsStr, OsStr?)? next();
}

/** Os-specific extensions for [`Command`] */
@rust("std::os::linux::process::CommandExt")
public interface CommandExt {
    @MutSelf @RustRefOut public Command create_pidfd(bool val);
}

/** An iterator over the fully resolved environment variables. */
@rust("std::process::CommandResolvedEnvs")
public class CommandResolvedEnvs {
    @MutSelf public (OsString, OsString)? next();
}

/** A single component of a path. */
@rust("std::path::Component")
@RustClone
public enum Component {
    Prefix(PrefixComponent), RootDir, CurDir, ParentDir, Normal(OsStr);

    @RustRefOut public OsStr as_os_str();
}

/** An iterator over the [`Component`]s of a [`Path`]. */
@rust("std::path::Components")
@RustClone
public class Components {
    @RustRefOut public Path as_path();
    @MutSelf public Component? next();
}

/** Helper trait for [`[T]::concat`](slice::concat). */
@rust("std::slice::Concat")
public interface Concat<Item> {
    public Self.Output concat(&Self slice);
}

/** A Condition Variable */
@rust("std::sync::Condvar")
public class Condvar {
    public Condvar();
    @RustBorrowsSelf public MutexGuard<T> wait<T>(MutexGuard<T> guard) throws PoisonError<T>;
    @RustBorrowsSelf public MutexGuard<T> wait_while<T, F>(MutexGuard<T> guard, (T) -> bool condition) throws PoisonError<T>;
    public (MutexGuard<T>, bool) wait_timeout_ms<T>(MutexGuard<T> guard, u32 ms) throws PoisonError<T>;
    public (MutexGuard<T>, WaitTimeoutResult) wait_timeout<T>(MutexGuard<T> guard, Duration dur) throws PoisonError<T>;
    public (MutexGuard<T>, WaitTimeoutResult) wait_timeout_while<T, F>(MutexGuard<T> guard, Duration dur, (T) -> bool condition) throws PoisonError<T>;
    public void notify_one();
    public void notify_all();
}

/** A clone-on-write smart pointer. */
@rust("std::borrow::Cow")
@RustClone
public enum Cow<B> implements ToOwned, ToString {
    Borrowed(B), Owned(B.Owned);

    public static bool is_borrowed(&Cow c);
    public static bool is_owned(&Cow c);
    @MutSelf @RustRefOut public B.Owned to_mut();
    public B.Owned into_owned();
}

/** A cursor over a `LinkedList`. */
@rust("std::collections::Cursor")
@RustClone
public class Cursor<T, A> implements ToOwned {
    public uint? index();
    @MutSelf public void move_next();
    @MutSelf public void move_prev();
    @RustRefOut public T? current();
    @RustRefOut public T? peek_next();
    @RustRefOut public T? peek_prev();
    @RustRefOut public T? front();
    @RustRefOut public T? back();
    @RustRefOut public LinkedList<T, A> as_list();
}

/** A cursor over a `LinkedList` with editing operations. */
@rust("std::collections::CursorMut")
public class CursorMut<T, A> {
    public uint? index();
    @MutSelf public void move_next();
    @MutSelf public void move_prev();
    @MutSelf @RustRefOut public T? current();
    @MutSelf @RustRefOut public T? peek_next();
    @MutSelf @RustRefOut public T? peek_prev();
    @RustBorrowsSelf public Cursor<T, A> as_cursor();
    @RustRefOut public LinkedList<T, A> as_list();
    @MutSelf public void splice_after(LinkedList<T> list);
    @MutSelf public void splice_before(LinkedList<T> list);
    @MutSelf public void insert_after(T item);
    @MutSelf public void insert_before(T item);
    @MutSelf public T? remove_current();
    @MutSelf public LinkedList<T, A>? remove_current_as_list();
    @MutSelf public LinkedList<T, A> split_after();
    @MutSelf public LinkedList<T, A> split_before();
    @MutSelf public void push_front(T elt);
    @MutSelf public void push_back(T elt);
    @MutSelf public T? pop_front();
    @MutSelf public T? pop_back();
    @RustRefOut public T? front();
    @MutSelf @RustRefOut public T? front_mut();
    @RustRefOut public T? back();
    @MutSelf @RustRefOut public T? back_mut();
}

/** A cursor over a `BTreeMap` with editing operations, and which allows */
@rust("std::collections::btree_map::CursorMutKey")
public class CursorMutKey<K, V, A> {
    @MutSelf public (K, V)? next();
    @MutSelf public (K, V)? prev();
    @MutSelf public (K, V)? peek_next();
    @MutSelf public (K, V)? peek_prev();
    @RustBorrowsSelf public Cursor<K, V> as_cursor();
    @MutSelf public unsafe void insert_after_unchecked(K key, V value);
    @MutSelf public unsafe void insert_before_unchecked(K key, V value);
    @MutSelf public void insert_after(K key, V value) throws UnorderedKeyError;
    @MutSelf public void insert_before(K key, V value) throws UnorderedKeyError;
    @MutSelf public (K, V)? remove_next();
    @MutSelf public (K, V)? remove_prev();
}

@rust("std::env::consts::DLL_EXTENSION")
public const String DLL_EXTENSION;

@rust("std::env::consts::DLL_PREFIX")
public const String DLL_PREFIX;

@rust("std::env::consts::DLL_SUFFIX")
public const String DLL_SUFFIX;

/** The default [`Hasher`] used by [`RandomState`]. */
@rust("std::hash::DefaultHasher")
@RustClone
public class DefaultHasher {
    public DefaultHasher();
}

/** A lazy iterator producing elements in the difference of `BTreeSet`s. */
@rust("std::collections::btree_set::Difference")
@RustClone
public class Difference<T, A> implements ToOwned {
    @MutSelf public T? next();
}

/** An object providing access to a directory on the filesystem. */
@rust("std::fs::Dir")
public class Dir implements AsHandle, AsRawHandle, FromRawHandle, IntoRawHandle {
    public static Dir open<P>(P path) throws Error;
    public File open_file<P>(P path) throws Error;
    public Metadata metadata() throws Error;
}

/** A builder used to create directories in various manners. */
@rust("std::fs::DirBuilder")
public class DirBuilder implements DirBuilderExt {
    public DirBuilder();
    @MutSelf @RustRefOut public DirBuilder recursive(bool recursive);
    public void create<P>(P path) throws Error;
}

/** Unix-specific extensions to [`fs::DirBuilder`]. */
@rust("std::os::unix::fs::DirBuilderExt")
public interface DirBuilderExt {
    @MutSelf @RustRefOut public Self mode(u32 mode);
}

/** Entries returned by the [`ReadDir`] iterator. */
@rust("std::fs::DirEntry")
public class DirEntry implements DirEntryExt, DirEntryExt2 {
    public PathBuf path();
    public Metadata metadata() throws Error;
    public FileType file_type() throws Error;
    public OsString file_name();
}

/** Unix-specific extension methods for [`fs::DirEntry`]. */
@rust("std::os::unix::fs::DirEntryExt")
public interface DirEntryExt {
    public ulong ino();
}

/** Unix-specific extension methods for [`fs::DirEntry`]. */
@rust("std::os::unix::fs::DirEntryExt2")
public interface DirEntryExt2 {
    @RustRefOut public OsStr file_name_ref();
}

/** Helper struct for safely printing paths with [`format!`] and `{}`. */
@rust("std::path::Display")
public class Display {
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Drain")
public class Drain<T, A> {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::DrainSorted")
public class DrainSorted<T, A> {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** A `Duration` type to represent a span of time, typically used for system */
@rust("std::time::Duration")
@RustClone
public class Duration implements Any, Clone, CloneToUninit, Copy, Debug, Default, Eq, Hash, Ord, StructuralPartialEq {
    public Duration(ulong secs, u32 nanos);
    @RustDefault public Duration();
    public static Duration from_secs(ulong secs);
    public static Duration from_millis(ulong millis);
    public static Duration from_micros(ulong micros);
    public static Duration from_nanos(ulong nanos);
    public static Duration from_nanos_u128(u128 nanos);
    public static Duration from_weeks(ulong weeks);
    public static Duration from_days(ulong days);
    public static Duration from_hours(ulong hours);
    public static Duration from_mins(ulong mins);
    public bool is_zero();
    public ulong as_secs();
    public u32 subsec_millis();
    public u32 subsec_micros();
    public u32 subsec_nanos();
    public u128 as_millis();
    public u128 as_micros();
    public u128 as_nanos();
    public Duration abs_diff(Duration other);
    public Duration? checked_add(Duration rhs);
    public Duration saturating_add(Duration rhs);
    public Duration? checked_sub(Duration rhs);
    public Duration saturating_sub(Duration rhs);
    public Duration? checked_mul(u32 rhs);
    public Duration saturating_mul(u32 rhs);
    public Duration? checked_div(u32 rhs);
    public double as_secs_f64();
    public float as_secs_f32();
    public double as_millis_f64();
    public float as_millis_f32();
    public static Duration from_secs_f64(double secs);
    public static Duration from_secs_f32(float secs);
    public Duration mul_f64(double rhs);
    public Duration mul_f32(float rhs);
    public Duration div_f64(double rhs);
    public Duration div_f32(float rhs);
    public double div_duration_f64(Duration rhs);
    public float div_duration_f32(Duration rhs);
    public u128 div_duration_floor(Duration rhs);
    public u128 div_duration_ceil(Duration rhs);
    public static Duration try_from_secs_f32(float secs) throws TryFromFloatSecsError;
    public static Duration try_from_secs_f64(double secs) throws TryFromFloatSecsError;
}

@rust("std::env::consts::EXE_EXTENSION")
public const String EXE_EXTENSION;

@rust("std::env::consts::EXE_SUFFIX")
public const String EXE_SUFFIX;

/** `Empty` ignores any data written via [`Write`], and will always be empty */
@rust("std::io::Empty")
@RustClone
public class Empty implements Any, Clone, CloneToUninit, Copy, Debug, Default {
    @RustDefault public Empty();
}

/** An iterator of [`u16`] over the string encoded as UTF-16. */
@rust("std::str::EncodeUtf16")
@RustClone
public class EncodeUtf16 implements Any, Clone, CloneToUninit, Debug, FusedIterator, IntoIterator, Iterator {
    @MutSelf public ushort? next();
}

/** Iterator returned by [`OsStrExt::encode_wide`]. */
@rust("std::os::windows::ffi::EncodeWide")
@RustClone
public class EncodeWide {
    @MutSelf public ushort? next();
}

/** A view into a single entry in a map, which may either be vacant or occupied. */
@rust("std::collections::btree_map::Entry")
public enum Entry<K, V, A> {
    Vacant(VacantEntry<K, V, A>), Occupied(OccupiedEntry<K, V, A>);

    @RustRefOut public V or_insert(V default);
    @RustRefOut public V or_insert_with<F>(() -> V default);
    @RustRefOut public V or_try_insert_with<F, E>(() -> Result<V, E> default) throws E;
    @RustRefOut public V or_insert_with_key<F>((K) -> V default);
    @RustRefOut public V or_try_insert_with_key<F, E>((K) -> Result<V, E> default) throws E;
    @RustRefOut public K key();
    public Entry and_modify<F>((V) -> void f);
    @RustBorrowsSelf public OccupiedEntry<K, V, A> insert_entry(V value);
    @RustRefOut public V or_default();
}

/** The error type for I/O operations of the [`Read`], [`Write`], [`Seek`], and */
@rust("std::io::Error")
public class Error {
    public Error(ErrorKind kind, E error);
    public static Error other<E>(E error);
    public static Error last_os_error();
    public static Error from_raw_os_error(RawOsError code);
    public RawOsError? raw_os_error();
    @RustRefOut public Error? get_ref();
    @MutSelf @RustRefOut public Error? get_mut();
    public Error? into_inner();
    public E downcast<E>() throws Error;
    public ErrorKind kind();
}

/** A list specifying general categories of I/O error. */
@rust("std::io::ErrorKind")
@RustClone
public enum ErrorKind implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Hash, Ord, StructuralPartialEq {
    NotFound, PermissionDenied, ConnectionRefused, ConnectionReset, HostUnreachable, NetworkUnreachable, ConnectionAborted, NotConnected, AddrInUse, AddrNotAvailable, NetworkDown, BrokenPipe, AlreadyExists, WouldBlock, NotADirectory, IsADirectory, DirectoryNotEmpty, ReadOnlyFilesystem, FilesystemLoop, StaleNetworkFileHandle, InvalidInput, InvalidData, TimedOut, WriteZero, StorageFull, NotSeekable, QuotaExceeded, FileTooLarge, ResourceBusy, ExecutableFileBusy, Deadlock, CrossesDevices, TooManyLinks, InvalidFilename, ArgumentListTooLong, Interrupted, Unsupported, UnexpectedEof, OutOfMemory, InProgress, Other
}

/** An iterator over the escaped version of a byte slice. */
@rust("std::slice::EscapeAscii")
@RustClone
public class EscapeAscii implements Any, Clone, CloneToUninit, Debug, Display, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public ubyte? next();
}

/** This type represents the status code the current process can return */
@rust("std::process::ExitCode")
@RustClone
public class ExitCode implements ExitCodeExt, Termination {
    @RustDefault public ExitCode();
    public never exit_process();
}

/** Windows-specific extensions to [`process::ExitCode`]. */
@rust("std::os::windows::process::ExitCodeExt")
public interface ExitCodeExt {
    public Self from_raw(u32 raw);
}

/** Describes the result of a process after it has terminated. */
@rust("std::process::ExitStatus")
@RustClone
public class ExitStatus implements ExitStatusExt {
    @RustDefault public ExitStatus();
    public void exit_ok() throws ExitStatusError;
    public bool success();
    public i32? code();
}

/** Describes the result of a process after it has failed */
@rust("std::process::ExitStatusError")
@RustClone
public class ExitStatusError implements ExitStatusExt {
    public i32? code();
    public NonZero<i32>? code_nonzero();
    public ExitStatus into_status();
}

/** Unix-specific extensions to [`process::ExitStatus`] and */
@rust("std::os::unix::process::ExitStatusExt")
public interface ExitStatusExt {
    public Self from_raw(i32 raw);
    public i32? signal();
    public bool core_dumped();
    public i32? stopped_signal();
    public bool continued();
    public i32 into_raw();
}

/** This `struct` is created by the [`extract_if`] method on [`LinkedList`]. */
@rust("std::collections::ExtractIf")
public class ExtractIf<T, F, A> {
    @MutSelf public T? next();
}

@rust("std::env::consts::FAMILY")
public const String FAMILY;

/** An object providing access to an open file on the filesystem. */
@rust("std::fs::File")
public class File implements AsFd, AsHandle, AsRawFd, AsRawHandle, FileExt, FromRawFd, FromRawHandle, IntoRawFd, IntoRawHandle, IsTerminal, Read, Seek, Write {
    public static File open<P>(P path) throws Error;
    public static BufReader<File> open_buffered<P>(P path) throws Error;
    public static File create<P>(P path) throws Error;
    public static BufWriter<File> create_buffered<P>(P path) throws Error;
    public static File create_new<P>(P path) throws Error;
    public static OpenOptions options();
    public void sync_all() throws Error;
    public void sync_data() throws Error;
    public void lock() throws Error;
    public void lock_shared() throws Error;
    public void try_lock() throws TryLockError;
    public void try_lock_shared() throws TryLockError;
    public void unlock() throws Error;
    public void set_len(ulong size) throws Error;
    public Metadata metadata() throws Error;
    public File try_clone() throws Error;
    public void set_permissions(Permissions perm) throws Error;
    public void set_times(FileTimes times) throws Error;
    public void set_modified(SystemTime time) throws Error;
}

/** Unix-specific extensions to [`fs::File`]. */
@rust("std::os::unix::fs::FileExt")
public interface FileExt {
    public uint read_at(&mut ubyte[] buf, ulong offset) throws Error;
    public uint read_vectored_at(&mut IoSliceMut[] bufs, ulong offset) throws Error;
    public void read_exact_at(&mut ubyte[] buf, ulong offset) throws Error;
    public void read_buf_at(BorrowedCursor<ubyte> buf, ulong offset) throws Error;
    public void read_buf_exact_at(BorrowedCursor<ubyte> buf, ulong offset) throws Error;
    public uint write_at(ubyte[] buf, ulong offset) throws Error;
    public uint write_vectored_at(IoSlice[] bufs, ulong offset) throws Error;
    public void write_all_at(ubyte[] buf, ulong offset) throws Error;
}

/** Representation of the various timestamps on a file. */
@rust("std::fs::FileTimes")
@RustClone
public class FileTimes implements FileTimesExt {
    public FileTimes();
    public FileTimes set_accessed(SystemTime t);
    public FileTimes set_modified(SystemTime t);
}

/** OS-specific extensions to [`fs::FileTimes`]. */
@rust("std::os::darwin::fs::FileTimesExt")
public interface FileTimesExt {
    public Self set_created(SystemTime t);
}

/** A structure representing a type of file with accessors for each file type. */
@rust("std::fs::FileType")
@RustClone
public class FileType implements FileTypeExt {
    public bool is_dir();
    public bool is_file();
    public bool is_symlink();
}

/** Unix-specific extensions for [`fs::FileType`]. */
@rust("std::os::unix::fs::FileTypeExt")
public interface FileTypeExt {
    public bool is_block_device();
    public bool is_char_device();
    public bool is_fifo();
    public bool is_socket();
}

/** A classification of floating point numbers. */
@rust("std::num::FpCategory")
@RustClone
public enum FpCategory implements Any, Clone, CloneToUninit, Copy, Debug, Eq, StructuralPartialEq {
    Nan, Infinite, Zero, Subnormal, Normal
}

/** An error indicating that no nul byte was present. */
@rust("std::ffi::FromBytesUntilNulError")
@RustClone
public class FromBytesUntilNulError implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Error, StructuralPartialEq {
}

/** An error indicating that a nul byte was not in the expected position. */
@rust("std::ffi::FromBytesWithNulError")
@RustClone
public enum FromBytesWithNulError implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Error, StructuralPartialEq {
    InteriorNul, NotNulTerminated
}

/** Trait for types that can be converted from a fixed-size byte array with a specified endianness */
@rust("std::io::FromEndianBytes")
public interface FromEndianBytes {
}

/** A trait to express the ability to construct an object from a raw file */
@rust("std::os::fd::FromRawFd")
public interface FromRawFd {
    public unsafe Self from_raw_fd(RawFd fd);
}

/** Constructs I/O objects from raw handles. */
@rust("std::os::windows::io::FromRawHandle")
public interface FromRawHandle {
    public unsafe Self from_raw_handle(RawHandle handle);
}

/** Creates I/O objects from raw sockets. */
@rust("std::os::windows::io::FromRawSocket")
public interface FromRawSocket {
    public unsafe Self from_raw_socket(RawSocket sock);
}

/** A possible error value when converting a `String` from a UTF-16 byte slice. */
@rust("std::string::FromUtf16Error")
public class FromUtf16Error implements ToString {
}

/** A possible error value when converting a `String` from a UTF-8 byte vector. */
@rust("std::string::FromUtf8Error")
@RustClone
public class FromUtf8Error implements ToOwned, ToString {
    @RustRefOut public ubyte[] as_bytes();
    public String into_utf8_lossy();
    public Vec<ubyte> into_bytes();
    public Utf8Error utf8_error();
}

/** An error indicating that a nul byte was not in the expected position. */
@rust("std::ffi::c_str::FromVecWithNulError")
@RustClone
public class FromVecWithNulError implements ToOwned, ToString {
    @RustRefOut public ubyte[] as_bytes();
    public Vec<ubyte> into_bytes();
}

/** The error type returned by [`get_disjoint_mut`][`slice::get_disjoint_mut`]. */
@rust("std::slice::GetDisjointMutError")
@RustClone
public enum GetDisjointMutError implements Any, Clone, CloneToUninit, Debug, Display, Eq, Error, StructuralPartialEq {
    IndexOutOfBounds, OverlappingIndices
}

/** The global memory allocator. */
@rust("std::alloc::Global")
@RustClone
public class Global implements ToOwned {
    @RustDefault public Global();
}

/** An owned container for `HANDLE` object, closing them on Drop. */
@rust("std::sys::pal::windows::handle::Handle")
public class Handle {
}

/** FFI type for handles in return values or out parameters, where `INVALID_HANDLE_VALUE` is used */
@rust("std::os::windows::io::HandleOrInvalid")
public class HandleOrInvalid {
    public static unsafe HandleOrInvalid from_raw_handle(RawHandle handle);
}

/** FFI type for handles in return values or out parameters, where `NULL` is used */
@rust("std::os::windows::io::HandleOrNull")
public class HandleOrNull {
    public static unsafe HandleOrNull from_raw_handle(RawHandle handle);
}

/** A [hash map] implemented with quadratic probing and SIMD lookup. */
@rust("std::collections::HashMap")
@RustIndexRef
@RustClone
@RustCollection
public class HashMap<K, V, S, A> {
    public HashMap();
    public static Map<K, V> with_capacity(uint capacity);
    public static HashMap new_in(A alloc);
    public static HashMap with_capacity_in(uint capacity, A alloc);
    public static Map<K, V> with_hasher(S hash_builder);
    public static Map<K, V> with_capacity_and_hasher(uint capacity, S hasher);
    public static HashMap with_hasher_in(S hash_builder, A alloc);
    public static HashMap with_capacity_and_hasher_in(uint capacity, S hash_builder, A alloc);
    public uint capacity();
    @RustBorrowsSelf public Keys<K, V> keys();
    public IntoKeys<K, V, A> into_keys();
    @RustBorrowsSelf public Values<K, V> values();
    @MutSelf @RustBorrowsSelf public ValuesMut<K, V> values_mut();
    public IntoValues<K, V, A> into_values();
    @RustBorrowsSelf public Iter<K, V> iter();
    @MutSelf @RustBorrowsSelf public IterMut<K, V> iter_mut();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBorrowsSelf public Drain<K, V, A> drain();
    @MutSelf @RustBorrowsSelf public ExtractIf<K, V, F, A> extract_if<F>((K, V) -> bool pred);
    @MutSelf public void retain<F>((K, V) -> bool f);
    @MutSelf public void clear();
    @RustRefOut public S hasher();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf @RustBorrowsSelf public Entry<K, V, A> entry(K key);
    @RustRefOut public V? get<Q>(&Q k);
    public (K, V)? get_key_value<Q>(&Q k);
    @MutSelf public V?[] get_disjoint_mut<Q>(Q[] ks);
    @MutSelf public unsafe V?[] get_disjoint_unchecked_mut<Q>(Q[] ks);
    public bool contains_key<Q>(&Q k);
    @MutSelf @RustRefOut public V? get_mut<Q>(&Q k);
    @MutSelf public V? insert(K k, V v);
    @MutSelf @RustBorrowsSelf @RustRefOut public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf public V? remove<Q>(&Q k);
    @MutSelf public (K, V)? remove_entry<Q>(&Q k);
}

/** A [hash set] implemented as a `HashMap` where the value is `()`. */
@rust("std::collections::HashSet")
@RustClone
@RustCollection
public class HashSet<T, S, A> {
    public HashSet();
    public static Set<T> with_capacity(uint capacity);
    public static Set<T> new_in(A alloc);
    public static Set<T> with_capacity_in(uint capacity, A alloc);
    public static Set<T> with_hasher(S hasher);
    public static Set<T> with_capacity_and_hasher(uint capacity, S hasher);
    public static Set<T> with_hasher_in(S hasher, A alloc);
    public static Set<T> with_capacity_and_hasher_in(uint capacity, S hasher, A alloc);
    public uint capacity();
    @RustBorrowsSelf public Iter<T> iter();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain();
    @MutSelf @RustBorrowsSelf public ExtractIf<T, F, A> extract_if<F>((T) -> bool pred);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void clear();
    @RustRefOut public S hasher();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @RustBorrowsSelf public Difference<T, S, A> difference(&Set<T> other);
    @RustBorrowsSelf public SymmetricDifference<T, S, A> symmetric_difference(&Set<T> other);
    @RustBorrowsSelf public Intersection<T, S, A> intersection(&Set<T> other);
    @RustBorrowsSelf public Union<T, S, A> union(&Set<T> other);
    public bool contains<Q>(&Q value);
    @RustRefOut public T? get<Q>(&Q value);
    @MutSelf @RustRefOut public T get_or_insert(T value);
    @MutSelf @RustRefOut public T get_or_insert_with<Q, F>(&Q value, (Q) -> T f);
    @MutSelf @RustBorrowsSelf public Entry<T, S, A> entry(T value);
    public bool is_disjoint(&Set<T> other);
    public bool is_subset(&Set<T> other);
    public bool is_superset(&Set<T> other);
    @MutSelf public bool insert(T value);
    @MutSelf public T? replace(T value);
    @MutSelf public bool remove<Q>(&Q value);
    @MutSelf public T? take<Q>(&Q value);
}

/** An iterator that infinitely [`accept`]s connections on a [`TcpListener`]. */
@rust("std::net::Incoming")
public class Incoming {
    @MutSelf public TcpStream? next() throws Error;
}

/** A measurement of a monotonically nondecreasing clock. */
@rust("std::time::Instant")
@RustClone
public class Instant {
    public static Instant now();
    public Duration duration_since(Instant earlier);
    public Duration? checked_duration_since(Instant earlier);
    public Duration saturating_duration_since(Instant earlier);
    public Duration elapsed();
    public Instant? checked_add(Duration duration);
    public Instant? checked_sub(Duration duration);
}

/** Enum to store the various types of errors that can cause parsing or converting an */
@rust("std::num::IntErrorKind")
@RustClone
public enum IntErrorKind implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, StructuralPartialEq {
    Empty, InvalidDigit, PosOverflow, NegOverflow, Zero, NotAPowerOfTwo
}

/** A lazy iterator producing elements in the intersection of `BTreeSet`s. */
@rust("std::collections::btree_set::Intersection")
@RustClone
public class Intersection<T, A> implements ToOwned {
    @MutSelf public T? next();
}

/** An iterator over the [`char`]s of a string. */
@rust("std::string::IntoChars")
@RustClone
public class IntoChars implements ToOwned {
    @RustRefOut public String as_str();
    public String into_string();
    @MutSelf public char? next();
}

/** An iterator that infinitely [`accept`]s connections on a [`TcpListener`]. */
@rust("std::net::IntoIncoming")
public class IntoIncoming {
    @MutSelf public TcpStream? next() throws Error;
}

/** An error returned by [`BufWriter::into_inner`] which combines an error that */
@rust("std::io::IntoInnerError")
public class IntoInnerError<W> {
    @RustRefOut public Error error();
    public W into_inner();
    public Error into_error();
    public (Error, W) into_parts();
}

/** An owning iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::IntoIter")
@RustClone
public class IntoIter<T, A> implements ToOwned {
    @RustDefault public IntoIter();
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

@rust("std::collections::IntoIterSorted")
@RustClone
public class IntoIterSorted<T, A> implements ToOwned {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** An owning iterator over the keys of a `BTreeMap`. */
@rust("std::collections::btree_map::IntoKeys")
public class IntoKeys<K, V, A> {
    @RustDefault public IntoKeys();
    @MutSelf public K? next();
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::fd::IntoRawFd")
public interface IntoRawFd {
    public RawFd into_raw_fd();
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::windows::io::IntoRawHandle")
public interface IntoRawHandle {
    public RawHandle into_raw_handle();
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::windows::io::IntoRawSocket")
public interface IntoRawSocket {
    public RawSocket into_raw_socket();
}

/** An error indicating invalid UTF-8 when converting a [`CString`] into a [`String`]. */
@rust("std::ffi::c_str::IntoStringError")
@RustClone
public class IntoStringError implements ToOwned, ToString {
    public CString into_cstring();
    public Utf8Error utf8_error();
}

/** An owning iterator over the values of a `BTreeMap`. */
@rust("std::collections::btree_map::IntoValues")
public class IntoValues<K, V, A> {
    @RustDefault public IntoValues();
    @MutSelf public V? next();
}

/** This is the error type used by [`HandleOrInvalid`] when attempting to */
@rust("std::os::windows::io::InvalidHandleError")
@RustClone
public class InvalidHandleError {
}

/** A buffer type used with `Write::write_vectored`. */
@rust("std::io::IoSlice")
@RustClone
public class IoSlice implements Any, Clone, CloneToUninit, Copy, Debug, Deref, Receiver, Send, Sync {
    public IoSlice(ubyte[] buf);
    @MutSelf public void advance(uint n);
    public static void advance_slices(&mut IoSlice[] bufs, uint n);
    @RustRefOut public ubyte[] as_slice();
    @MutSelf public void sort_floats();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
    public bool is_ascii();
    @RustRefOut public Char[]? as_ascii();
    @RustRefOut public unsafe Char[] as_ascii_unchecked();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    public uint len();
    public bool is_empty();
    @RustRefOut public T? first();
    @MutSelf @RustRefOut public T? first_mut();
    public (T, T[])? split_first();
    @MutSelf public (T, T[])? split_first_mut();
    public (T, T[])? split_last();
    @MutSelf public (T, T[])? split_last_mut();
    @RustRefOut public T? last();
    @MutSelf @RustRefOut public T? last_mut();
    @RustRefOut public T[]? first_chunk();
    @MutSelf @RustRefOut public T[]? first_chunk_mut();
    public (T[], T[])? split_first_chunk();
    @MutSelf public (T[], T[])? split_first_chunk_mut();
    public (T[], T[])? split_last_chunk();
    @MutSelf public (T[], T[])? split_last_chunk_mut();
    @RustRefOut public T[]? last_chunk();
    @MutSelf @RustRefOut public T[]? last_chunk_mut();
    @RustRefOut public I.Output? get<I>(I index);
    @MutSelf @RustRefOut public I.Output? get_mut<I>(I index);
    @RustRefOut public unsafe I.Output get_unchecked<I>(I index);
    @MutSelf @RustRefOut public unsafe I.Output get_unchecked_mut<I>(I index);
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    public Range<T*> as_ptr_range();
    @MutSelf public Range<T*> as_mut_ptr_range();
    @RustRefOut public T[]? as_array();
    @MutSelf @RustRefOut public T[]? as_mut_array();
    @MutSelf public void swap(uint a, uint b);
    @MutSelf public unsafe void swap_unchecked(uint a, uint b);
    @MutSelf public void reverse();
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    @RustBorrowsSelf public Windows<T> windows(uint size);
    @RustBorrowsSelf public Chunks<T> chunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksMut<T> chunks_mut(uint chunk_size);
    @RustBorrowsSelf public ChunksExact<T> chunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksExactMut<T> chunks_exact_mut(uint chunk_size);
    @RustRefOut public unsafe T[][] as_chunks_unchecked();
    public (T[][], T[]) as_chunks();
    public (T[], T[][]) as_rchunks();
    @MutSelf @RustRefOut public unsafe T[][] as_chunks_unchecked_mut();
    @MutSelf public (T[][], T[]) as_chunks_mut();
    @MutSelf public (T[], T[][]) as_rchunks_mut();
    @RustBorrowsSelf public ArrayWindows<T> array_windows();
    @RustBorrowsSelf public RChunks<T> rchunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksMut<T> rchunks_mut(uint chunk_size);
    @RustBorrowsSelf public RChunksExact<T> rchunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksExactMut<T> rchunks_exact_mut(uint chunk_size);
    @RustBorrowsSelf public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    public (T[], T[])? split_once<F>((T) -> bool pred);
    public (T[], T[])? rsplit_once<F>((T) -> bool pred);
    public bool contains(&T x);
    public bool starts_with(T[] needle);
    public bool ends_with(T[] needle);
    @RustRefOut public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut public T[]? strip_suffix<P>(&P suffix);
    @RustRefOut public T[]? strip_circumfix<S, P>(&P prefix, &S suffix);
    @RustRefOut public T[] trim_prefix<P>(&P prefix);
    @RustRefOut public T[] trim_suffix<P>(&P suffix);
    public uint binary_search(&T x) throws Error;
    public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf public void sort_unstable();
    @MutSelf public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf public void partial_sort_unstable<R>(R range);
    @MutSelf public void partial_sort_unstable_by<F, R>(R range, (T, T) -> Ordering compare);
    @MutSelf public void partial_sort_unstable_by_key<K, F, R>(R range, (T) -> K f);
    @MutSelf public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public (T[], T[]) partition_dedup();
    @MutSelf public (T[], T[]) partition_dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public (T[], T[]) partition_dedup_by_key<K, F>((T) -> K key);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf public T[] shift_left(T[] inserted);
    @MutSelf public T[] shift_right(T[] inserted);
    @MutSelf public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf public void clone_from_slice(T[] src);
    @MutSelf public void copy_from_slice(T[] src);
    @MutSelf public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    public (T[], Simd<T>[], T[]) as_simd();
    @MutSelf public (T[], Simd<T>[], T[]) as_simd_mut();
    public bool is_sorted();
    public bool is_sorted_by<F>((T, T) -> bool compare);
    public bool is_sorted_by_key<F, K>((T) -> K f);
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off<R>(R range);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public Range<uint>? subslice_range(T[] subslice);
    @MutSelf @RustRefOut public T[] as_mut_slice();
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
}

/** A buffer type used with `Read::read_vectored`. */
@rust("std::io::IoSliceMut")
public class IoSliceMut implements Any, Debug, Deref, DerefMut, Receiver, Send, Sync {
    public IoSliceMut(&mut ubyte[] buf);
    @MutSelf public void advance(uint n);
    public static void advance_slices(&mut IoSliceMut[] bufs, uint n);
    @RustRefOut public ubyte[] into_slice();
    @MutSelf public void sort_floats();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
    public bool is_ascii();
    @RustRefOut public Char[]? as_ascii();
    @RustRefOut public unsafe Char[] as_ascii_unchecked();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    public uint len();
    public bool is_empty();
    @RustRefOut public T? first();
    @MutSelf @RustRefOut public T? first_mut();
    public (T, T[])? split_first();
    @MutSelf public (T, T[])? split_first_mut();
    public (T, T[])? split_last();
    @MutSelf public (T, T[])? split_last_mut();
    @RustRefOut public T? last();
    @MutSelf @RustRefOut public T? last_mut();
    @RustRefOut public T[]? first_chunk();
    @MutSelf @RustRefOut public T[]? first_chunk_mut();
    public (T[], T[])? split_first_chunk();
    @MutSelf public (T[], T[])? split_first_chunk_mut();
    public (T[], T[])? split_last_chunk();
    @MutSelf public (T[], T[])? split_last_chunk_mut();
    @RustRefOut public T[]? last_chunk();
    @MutSelf @RustRefOut public T[]? last_chunk_mut();
    @RustRefOut public I.Output? get<I>(I index);
    @MutSelf @RustRefOut public I.Output? get_mut<I>(I index);
    @RustRefOut public unsafe I.Output get_unchecked<I>(I index);
    @MutSelf @RustRefOut public unsafe I.Output get_unchecked_mut<I>(I index);
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    public Range<T*> as_ptr_range();
    @MutSelf public Range<T*> as_mut_ptr_range();
    @RustRefOut public T[]? as_array();
    @MutSelf @RustRefOut public T[]? as_mut_array();
    @MutSelf public void swap(uint a, uint b);
    @MutSelf public unsafe void swap_unchecked(uint a, uint b);
    @MutSelf public void reverse();
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    @RustBorrowsSelf public Windows<T> windows(uint size);
    @RustBorrowsSelf public Chunks<T> chunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksMut<T> chunks_mut(uint chunk_size);
    @RustBorrowsSelf public ChunksExact<T> chunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksExactMut<T> chunks_exact_mut(uint chunk_size);
    @RustRefOut public unsafe T[][] as_chunks_unchecked();
    public (T[][], T[]) as_chunks();
    public (T[], T[][]) as_rchunks();
    @MutSelf @RustRefOut public unsafe T[][] as_chunks_unchecked_mut();
    @MutSelf public (T[][], T[]) as_chunks_mut();
    @MutSelf public (T[], T[][]) as_rchunks_mut();
    @RustBorrowsSelf public ArrayWindows<T> array_windows();
    @RustBorrowsSelf public RChunks<T> rchunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksMut<T> rchunks_mut(uint chunk_size);
    @RustBorrowsSelf public RChunksExact<T> rchunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksExactMut<T> rchunks_exact_mut(uint chunk_size);
    @RustBorrowsSelf public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    public (T[], T[])? split_once<F>((T) -> bool pred);
    public (T[], T[])? rsplit_once<F>((T) -> bool pred);
    public bool contains(&T x);
    public bool starts_with(T[] needle);
    public bool ends_with(T[] needle);
    @RustRefOut public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut public T[]? strip_suffix<P>(&P suffix);
    @RustRefOut public T[]? strip_circumfix<S, P>(&P prefix, &S suffix);
    @RustRefOut public T[] trim_prefix<P>(&P prefix);
    @RustRefOut public T[] trim_suffix<P>(&P suffix);
    public uint binary_search(&T x) throws Error;
    public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf public void sort_unstable();
    @MutSelf public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf public void partial_sort_unstable<R>(R range);
    @MutSelf public void partial_sort_unstable_by<F, R>(R range, (T, T) -> Ordering compare);
    @MutSelf public void partial_sort_unstable_by_key<K, F, R>(R range, (T) -> K f);
    @MutSelf public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public (T[], T[]) partition_dedup();
    @MutSelf public (T[], T[]) partition_dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public (T[], T[]) partition_dedup_by_key<K, F>((T) -> K key);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf public T[] shift_left(T[] inserted);
    @MutSelf public T[] shift_right(T[] inserted);
    @MutSelf public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf public void clone_from_slice(T[] src);
    @MutSelf public void copy_from_slice(T[] src);
    @MutSelf public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    public (T[], Simd<T>[], T[]) as_simd();
    @MutSelf public (T[], Simd<T>[], T[]) as_simd_mut();
    public bool is_sorted();
    public bool is_sorted_by<F>((T, T) -> bool compare);
    public bool is_sorted_by_key<F, K>((T) -> K f);
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off<R>(R range);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public Range<uint>? subslice_range(T[] subslice);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public T[] as_mut_slice();
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
}

/** An IP address, either IPv4 or IPv6. */
@rust("std::net::IpAddr")
@RustClone
public enum IpAddr implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, FromStr, Hash, Ord, StructuralPartialEq {
    V4(Ipv4Addr), V6(Ipv6Addr);

    public bool is_unspecified();
    public bool is_loopback();
    public bool is_global();
    public bool is_multicast();
    public bool is_documentation();
    public bool is_benchmarking();
    public bool is_ipv4();
    public bool is_ipv6();
    public IpAddr to_canonical();
    @RustRefOut public ubyte[] as_octets();
    public static IpAddr parse_ascii(ubyte[] b) throws AddrParseError;
}

/** An IPv4 address. */
@rust("std::net::Ipv4Addr")
@RustClone
public class Ipv4Addr implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, FromStr, Hash, Not, Ord, Step, StructuralPartialEq, TrustedStep {
    public Ipv4Addr(ubyte a, ubyte b, ubyte c, ubyte d);
    public u32 to_bits();
    public static Ipv4Addr from_bits(u32 bits);
    public ubyte[4] octets();
    public static Ipv4Addr from_octets(ubyte[4] octets);
    @RustRefOut public ubyte[4] as_octets();
    public bool is_unspecified();
    public bool is_loopback();
    public bool is_private();
    public bool is_link_local();
    public bool is_global();
    public bool is_shared();
    public bool is_benchmarking();
    public bool is_reserved();
    public bool is_multicast();
    public bool is_broadcast();
    public bool is_documentation();
    public Ipv6Addr to_ipv6_compatible();
    public Ipv6Addr to_ipv6_mapped();
    public static Ipv4Addr parse_ascii(ubyte[] b) throws AddrParseError;
}

/** An IPv6 address. */
@rust("std::net::Ipv6Addr")
@RustClone
public class Ipv6Addr implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, FromStr, Hash, Not, Ord, Step, StructuralPartialEq, TrustedStep {
    public Ipv6Addr(ushort a, ushort b, ushort c, ushort d, ushort e, ushort f, ushort g, ushort h);
    public u128 to_bits();
    public static Ipv6Addr from_bits(u128 bits);
    public ushort[8] segments();
    public static Ipv6Addr from_segments(ushort[8] segments);
    public bool is_unspecified();
    public bool is_loopback();
    public bool is_global();
    public bool is_unique_local();
    public bool is_unicast();
    public bool is_unicast_link_local();
    public bool is_documentation();
    public bool is_benchmarking();
    public bool is_unicast_global();
    public Ipv6MulticastScope? multicast_scope();
    public bool is_multicast();
    public bool is_ipv4_mapped();
    public Ipv4Addr? to_ipv4_mapped();
    public Ipv4Addr? to_ipv4();
    public IpAddr to_canonical();
    public ubyte[16] octets();
    public static Ipv6Addr from_octets(ubyte[16] octets);
    @RustRefOut public ubyte[16] as_octets();
    public static Ipv6Addr parse_ascii(ubyte[] b) throws AddrParseError;
}

/** Scope of an [IPv6 multicast address] as defined in [IETF RFC 7346 section 2], */
@rust("std::net::Ipv6MulticastScope")
@RustClone
public enum Ipv6MulticastScope implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, Ord, StructuralPartialEq {
    InterfaceLocal = 1, LinkLocal = 2, RealmLocal = 3, AdminLocal = 4, SiteLocal = 5, Unassigned6 = 6, Unassigned7 = 7, OrganizationLocal = 8, Unassigned9 = 9, UnassignedA = 10, UnassignedB = 11, UnassignedC = 12, UnassignedD = 13, Global = 14
}

/** Trait to determine if a descriptor/handle refers to a terminal/tty. */
@rust("std::io::IsTerminal")
public interface IsTerminal {
    public bool is_terminal();
}

/** An iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Iter")
@RustClone
public class Iter<T> implements ToOwned {
    @RustDefault public Iter();
    @MutSelf public T? next();
}

/** A mutable iterator over the elements of a `LinkedList`. */
@rust("std::collections::IterMut")
public class IterMut<T> {
    @RustDefault public IterMut();
    @MutSelf public T? next();
}

/** Helper trait for [`[T]::join`](slice::join) */
@rust("std::slice::Join")
public interface Join<Separator> {
    public Self.Output join(&Self slice, Separator sep);
}

/** An owned permission to join on a thread (block on its termination). */
@rust("std::thread::JoinHandle")
public class JoinHandle<T> implements AsHandle, AsRawHandle, IntoRawHandle, JoinHandleExt {
    @RustRefOut public Thread thread();
    public T join() throws Error;
    public bool is_finished();
}

/** Unix-specific extensions to [`JoinHandle`]. */
@rust("std::os::unix::thread::JoinHandleExt")
public interface JoinHandleExt {
    public RawPthread as_pthread_t();
    public RawPthread into_pthread_t();
}

/** The error type for operations on the `PATH` variable. Possibly returned from */
@rust("std::env::JoinPathsError")
public class JoinPathsError {
}

/** An iterator over the keys of a `BTreeMap`. */
@rust("std::collections::btree_map::Keys")
@RustClone
public class Keys<K, V> implements ToOwned {
    @RustDefault public Keys();
    @MutSelf public K? next();
}

/** Layout of a block of memory. */
@rust("std::alloc::Layout")
@RustClone
public class Layout implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, StructuralPartialEq {
    public Layout();
    public static Layout from_size_align(uint size, uint align) throws LayoutError;
    public static Layout from_size_alignment(uint size, Alignment alignment) throws LayoutError;
    public static unsafe Layout from_size_align_unchecked(uint size, uint align);
    public static unsafe Layout from_size_alignment_unchecked(uint size, Alignment alignment);
    public uint size();
    public uint align();
    public Alignment alignment();
    public static Layout for_value<T>(&T t);
    public static unsafe Layout for_value_raw<T>(T* t);
    public NonNull<ubyte> dangling_ptr();
    public Layout align_to(uint align) throws LayoutError;
    public Layout adjust_alignment_to(Alignment alignment) throws LayoutError;
    public uint padding_needed_for(Alignment alignment);
    public Layout pad_to_align();
    public (Layout, uint) repeat(uint n) throws LayoutError;
    public (Layout, uint) extend(Layout next) throws LayoutError;
    public Layout repeat_packed(uint n) throws LayoutError;
    public Layout extend_packed(Layout next) throws LayoutError;
    public static Layout array<T>(uint n) throws LayoutError;
}

/** A value which is initialized on the first access. */
@rust("std::sync::LazyLock")
public class LazyLock<T, F> {
    public LazyLock(F f);
    @RustDefault public LazyLock();
    public static T into_inner(LazyLock this) throws F;
    @RustRefOut public static T force_mut(&mut LazyLock<T, F> this);
    @RustRefOut public static T force(&LazyLock<T, F> this);
    @RustRefOut public static T? get_mut(&mut LazyLock<T, F> this);
    @RustRefOut public static T? get(&LazyLock<T, F> this);
}

/** Wraps a writer and buffers output to it, flushing whenever a newline */
@rust("std::io::LineWriter")
public class LineWriter<W> implements Write {
    public LineWriter(W inner);
    public static LineWriter<W> with_capacity(uint capacity, W inner);
    @MutSelf @RustRefOut public W get_mut();
    public W into_inner() throws IntoInnerError<LineWriter<W>>;
    @RustRefOut public W get_ref();
}

/** An iterator over the lines of an instance of `BufRead`. */
@rust("std::io::Lines")
public class Lines<B> {
    @MutSelf public String? next() throws Error;
}

/** Created with the method [`lines_any`]. */
@rust("std::str::LinesAny")
@RustClone
public class LinesAny implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public String? next();
}

/** A doubly-linked list with owned nodes. */
@rust("std::collections::LinkedList")
@RustClone
@RustCollection
public class LinkedList<T, A> implements ToOwned {
    public LinkedList();
    @MutSelf public void append(&mut LinkedList other);
    public static LinkedList new_in(A alloc);
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    @RustBorrowsSelf public Cursor<T, A> cursor_front();
    @MutSelf @RustBorrowsSelf public CursorMut<T, A> cursor_front_mut();
    @RustBorrowsSelf public Cursor<T, A> cursor_back();
    @MutSelf @RustBorrowsSelf public CursorMut<T, A> cursor_back_mut();
    public bool is_empty();
    public uint len();
    @MutSelf public void clear();
    public bool contains(&T x);
    @RustRefOut public T? front();
    @MutSelf @RustRefOut public T? front_mut();
    @RustRefOut public T? back();
    @MutSelf @RustRefOut public T? back_mut();
    @MutSelf public void push_front(T elt);
    @MutSelf @RustRefOut public T push_front_mut(T elt);
    @MutSelf public T? pop_front();
    @MutSelf public void push_back(T elt);
    @MutSelf @RustRefOut public T push_back_mut(T elt);
    @MutSelf public T? pop_back();
    @MutSelf public LinkedList<T, A> split_off(uint at);
    @MutSelf public T remove(uint at);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf @RustBorrowsSelf public ExtractIf<T, F, A> extract_if<F>((T) -> bool filter);
}

/** A thread local storage (TLS) key which owns its contents. */
@rust("std::thread::LocalKey")
public class LocalKey<T> {
    public R with<F, R>((T) -> R f);
    public R try_with<F, R>((T) -> R f) throws AccessError;
    public void set(T value);
    public T get();
    public T take();
    public T replace(T value);
    public void update((T) -> T f);
    public R with_borrow<F, R>((T) -> R f);
    public R with_borrow_mut<F, R>((T) -> R f);
}

/** An analogous trait to `Wake` but used to construct a `LocalWaker`. */
@rust("std::task::LocalWake")
public interface LocalWake {
    public void wake();
    public void wake_by_ref();
}

/** A `LocalWaker` is analogous to a [`Waker`], but it does not implement [`Send`] or [`Sync`]. */
@rust("std::task::LocalWaker")
@RustClone
public class LocalWaker implements Any, Clone, CloneToUninit, Debug, Drop, Unpin {
    public LocalWaker(Object* data, &RawWakerVTable vtable);
    public void wake();
    public void wake_by_ref();
    public bool will_wake(&LocalWaker other);
    public static unsafe LocalWaker from_raw(RawWaker waker);
    @RustRefOut public static LocalWaker noop();
    public Object* data();
    @RustRefOut public RawWakerVTable vtable();
    public static LocalWaker from_fn_ptr(() -> void f);
}

/** A struct containing information about the location of a panic. */
@rust("std::panic::Location")
@RustClone
public class Location implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Hash, Ord, Send, Sync {
    @RustRefOut public static Location caller();
    @RustRefOut public String file();
    @RustRefOut public CStr file_as_c_str();
    public u32 line();
    public u32 column();
}

@rust("std::path::MAIN_SEPARATOR")
public const char MAIN_SEPARATOR;

@rust("std::path::MAIN_SEPARATOR_STR")
public const String MAIN_SEPARATOR_STR;

/** An iterator that maps the values of `iter` with `f`. */
@rust("std::iter::Map")
@RustClone
public class Map<I, F> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @MutSelf public B? next();
}

/** An RAII mutex guard returned by `MutexGuard::map`, which can point to a */
@rust("std::sync::MappedMutexGuard")
public class MappedMutexGuard<T> {
    public static MappedMutexGuard<U> map<U, F>(MappedMutexGuard orig, (T) -> U f);
    public static MappedMutexGuard<U> filter_map<U, F>(MappedMutexGuard orig, (T) -> U? f) throws MappedMutexGuard;
}

/** RAII structure used to release the shared read access of a lock when */
@rust("std::sync::MappedRwLockReadGuard")
public class MappedRwLockReadGuard<T> {
    public static MappedRwLockReadGuard<U> map<U, F>(MappedRwLockReadGuard orig, (T) -> U f);
    public static MappedRwLockReadGuard<U> filter_map<U, F>(MappedRwLockReadGuard orig, (T) -> U? f) throws MappedRwLockReadGuard;
}

/** RAII structure used to release the exclusive write access of a lock when */
@rust("std::sync::MappedRwLockWriteGuard")
public class MappedRwLockWriteGuard<T> {
    public static MappedRwLockWriteGuard<U> map<U, F>(MappedRwLockWriteGuard orig, (T) -> U f);
    public static MappedRwLockWriteGuard<U> filter_map<U, F>(MappedRwLockWriteGuard orig, (T) -> U? f) throws MappedRwLockWriteGuard;
}

/** Created with the method [`match_indices`]. */
@rust("std::str::MatchIndices")
@RustClone
public class MatchIndices<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public (uint, String)? next();
}

/** Created with the method [`matches`]. */
@rust("std::str::Matches")
@RustClone
public class Matches<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public String? next();
}

/** This struct is used to iterate through the control messages. */
@rust("std::os::unix::net::Messages")
public class Messages {
    @MutSelf public AncillaryData? next() throws AncillaryError;
}

/** Metadata information about a file. */
@rust("std::fs::Metadata")
@RustClone
public class Metadata implements MetadataExt {
    public FileType file_type();
    public bool is_dir();
    public bool is_file();
    public bool is_symlink();
    public ulong len();
    public Permissions permissions();
    public SystemTime modified() throws Error;
    public SystemTime accessed() throws Error;
    public SystemTime created() throws Error;
}

/** OS-specific extensions to [`fs::Metadata`]. */
@rust("std::os::darwin::fs::MetadataExt")
public interface MetadataExt {
    @RustRefOut public stat as_raw_stat();
    public ulong st_dev();
    public ulong st_ino();
    public u32 st_mode();
    public ulong st_nlink();
    public u32 st_uid();
    public u32 st_gid();
    public ulong st_rdev();
    public ulong st_size();
    public long st_atime();
    public long st_atime_nsec();
    public long st_mtime();
    public long st_mtime_nsec();
    public long st_ctime();
    public long st_ctime_nsec();
    public long st_birthtime();
    public long st_birthtime_nsec();
    public ulong st_blksize();
    public ulong st_blocks();
    public u32 st_flags();
    public u32 st_gen();
    public u32 st_lspare();
}

/** A mutual exclusion primitive useful for protecting shared data */
@rust("std::sync::Mutex")
public class Mutex<T> {
    public Mutex(T t);
    @RustDefault public Mutex();
    public T get_cloned() throws PoisonError<void>;
    public void set(T value) throws PoisonError<T>;
    public T replace(T value) throws PoisonError<T>;
    @RustBorrowsSelf public MutexGuard<T> lock() throws PoisonError<T>;
    @RustBorrowsSelf public MutexGuard<T> try_lock() throws TryLockError<Guard>;
    public bool is_poisoned();
    public void clear_poison();
    public T into_inner() throws PoisonError<T>;
    @MutSelf @RustRefOut public T get_mut() throws PoisonError<T>;
    public T* data_ptr();
}

/** An RAII implementation of a "scoped lock" of a mutex. When this structure is */
@rust("std::sync::MutexGuard")
public class MutexGuard<T> {
    public static MappedMutexGuard<U> map<U, F>(MutexGuard orig, (T) -> U f);
    public static MappedMutexGuard<U> filter_map<U, F>(MutexGuard orig, (T) -> U? f) throws MutexGuard;
}

/** `*mut T` but non-zero and [covariant]. */
@rust("std::ptr::NonNull")
@RustClone
public class NonNull<T> implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, Ord, Pointer, UnwindSafe {
    public static NonNull without_provenance(NonZero<uint> addr);
    public static NonNull dangling();
    public static NonNull with_exposed_provenance(NonZero<uint> addr);
    @RustRefOut public unsafe MaybeUninit<T> as_uninit_ref();
    @RustRefOut public unsafe MaybeUninit<T> as_uninit_mut();
    public NonNull<T[]> cast_array();
    public static unsafe NonNull new_unchecked(T* ptr);
    public static NonNull? new(T* ptr);
    public static NonNull from_ref(&T r);
    public static NonNull from_mut(&mut T r);
    public static NonNull<T> from_raw_parts(NonNull<Thin> data_pointer, T.Metadata metadata);
    public (NonNull<void>, T.Metadata) to_raw_parts();
    public NonZero<uint> addr();
    public NonZero<uint> expose_provenance();
    public NonNull with_addr(NonZero<uint> addr);
    public NonNull map_addr((NonZero<uint>) -> NonZero<uint> f);
    public T* as_ptr();
    @RustRefOut public unsafe T as_ref();
    @MutSelf @RustRefOut public unsafe T as_mut();
    public NonNull<U> cast<U>();
    public NonNull<U>? try_cast_aligned<U>();
    public unsafe NonNull offset(int count);
    public unsafe NonNull byte_offset(int count);
    public unsafe NonNull add(uint count);
    public unsafe NonNull byte_add(uint count);
    public unsafe NonNull sub(uint count);
    public unsafe NonNull byte_sub(uint count);
    public unsafe int offset_from(NonNull<T> origin);
    public unsafe int byte_offset_from<U>(NonNull<U> origin);
    public unsafe uint offset_from_unsigned(NonNull<T> subtracted);
    public unsafe uint byte_offset_from_unsigned<U>(NonNull<U> origin);
    public unsafe T read();
    public unsafe T read_volatile();
    public unsafe T read_unaligned();
    public unsafe void copy_to(NonNull<T> dest, uint count);
    public unsafe void copy_to_nonoverlapping(NonNull<T> dest, uint count);
    public unsafe void copy_from(NonNull<T> src, uint count);
    public unsafe void copy_from_nonoverlapping(NonNull<T> src, uint count);
    public unsafe void drop_in_place();
    public unsafe void write(T val);
    public unsafe void write_bytes(ubyte val, uint count);
    public unsafe void write_volatile(T val);
    public unsafe void write_unaligned(T val);
    public unsafe T replace(T src);
    public unsafe void swap(NonNull<T> with);
    public uint align_offset(uint align);
    public bool is_aligned();
    public bool is_aligned_to(uint align);
    public NonNull<MaybeUninit<T>> cast_uninit();
    public NonNull<T[]> cast_slice(uint len);
    public NonNull<T> cast_init();
    public static NonNull slice_from_raw_parts(NonNull<T> data, uint len);
    public uint len();
    public bool is_empty();
    public NonNull<T> as_non_null_ptr();
    public T* as_mut_ptr();
    @RustRefOut public unsafe MaybeUninit<T>[] as_uninit_slice();
    @RustRefOut public unsafe MaybeUninit<T>[] as_uninit_slice_mut();
    public unsafe NonNull<I.Output> get_unchecked_mut<I>(I index);
}

/** A value that is known not to equal zero. */
@rust("std::num::NonZero")
@RustClone
public class NonZero<T> implements Any, Binary, Clone, CloneToUninit, Copy, Debug, Display, Eq, Freeze, Hash, LowerExp, LowerHex, Octal, Ord, RefUnwindSafe, Send, StructuralPartialEq, Sync, Unpin, UnwindSafe, UpperExp, UpperHex, UseCloned {
    public static NonZero? new(T n);
    public static unsafe NonZero new_unchecked(T n);
    @RustRefOut public static NonZero? from_mut(&mut T n);
    @RustRefOut public static unsafe NonZero from_mut_unchecked(&mut T n);
    public T get();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero isolate_highest_one();
    public NonZero isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZero rotate_left(u32 n);
    public NonZero rotate_right(u32 n);
    public NonZero swap_bytes();
    public NonZero reverse_bits();
    public static NonZero from_be(NonZero x);
    public static NonZero from_le(NonZero x);
    public NonZero to_be();
    public NonZero to_le();
    public NonZero? checked_add(ubyte other);
    public NonZero saturating_add(ubyte other);
    public unsafe NonZero unchecked_add(ubyte other);
    public NonZero? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZero midpoint(NonZero rhs);
    public bool is_power_of_two();
    public NonZero isqrt();
    public NonZero<byte> cast_signed();
    public NonZero<u32> bit_width();
    public NonZero? checked_mul(NonZero other);
    public NonZero saturating_mul(NonZero other);
    public unsafe NonZero unchecked_mul(NonZero other);
    public NonZero? checked_pow(u32 other);
    public NonZero saturating_pow(u32 other);
    public static NonZero from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZero from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZero from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZero div_ceil(NonZero rhs);
    public NonZero abs();
    public NonZero? checked_abs();
    public (NonZero, bool) overflowing_abs();
    public NonZero saturating_abs();
    public NonZero wrapping_abs();
    public NonZero<ubyte> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZero? checked_neg();
    public (NonZero, bool) overflowing_neg();
    public NonZero saturating_neg();
    public NonZero wrapping_neg();
    public NonZero<ubyte> cast_unsigned();
}

/** An [`i128`] that is known not to equal zero. */
@rust("std::num::NonZeroI128")
public class NonZeroI128 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroI128 isolate_highest_one();
    public NonZeroI128 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroI128 rotate_left(u32 n);
    public NonZeroI128 rotate_right(u32 n);
    public NonZeroI128 swap_bytes();
    public NonZeroI128 reverse_bits();
    public static NonZeroI128 from_be(NonZeroI128 x);
    public static NonZeroI128 from_le(NonZeroI128 x);
    public NonZeroI128 to_be();
    public NonZeroI128 to_le();
    public NonZeroI128 abs();
    public NonZeroI128? checked_abs();
    public (NonZeroI128, bool) overflowing_abs();
    public NonZeroI128 saturating_abs();
    public NonZeroI128 wrapping_abs();
    public NonZero<u128> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroI128? checked_neg();
    public (NonZeroI128, bool) overflowing_neg();
    public NonZeroI128 saturating_neg();
    public NonZeroI128 wrapping_neg();
    public NonZero<u128> cast_unsigned();
    public NonZeroI128? checked_mul(NonZeroI128 other);
    public NonZeroI128 saturating_mul(NonZeroI128 other);
    public unsafe NonZeroI128 unchecked_mul(NonZeroI128 other);
    public NonZeroI128? checked_pow(u32 other);
    public NonZeroI128 saturating_pow(u32 other);
    public static NonZeroI128 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroI128 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroI128 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An [`i16`] that is known not to equal zero. */
@rust("std::num::NonZeroI16")
public class NonZeroI16 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroI16 isolate_highest_one();
    public NonZeroI16 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroI16 rotate_left(u32 n);
    public NonZeroI16 rotate_right(u32 n);
    public NonZeroI16 swap_bytes();
    public NonZeroI16 reverse_bits();
    public static NonZeroI16 from_be(NonZeroI16 x);
    public static NonZeroI16 from_le(NonZeroI16 x);
    public NonZeroI16 to_be();
    public NonZeroI16 to_le();
    public NonZeroI16 abs();
    public NonZeroI16? checked_abs();
    public (NonZeroI16, bool) overflowing_abs();
    public NonZeroI16 saturating_abs();
    public NonZeroI16 wrapping_abs();
    public NonZero<ushort> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroI16? checked_neg();
    public (NonZeroI16, bool) overflowing_neg();
    public NonZeroI16 saturating_neg();
    public NonZeroI16 wrapping_neg();
    public NonZero<ushort> cast_unsigned();
    public NonZeroI16? checked_mul(NonZeroI16 other);
    public NonZeroI16 saturating_mul(NonZeroI16 other);
    public unsafe NonZeroI16 unchecked_mul(NonZeroI16 other);
    public NonZeroI16? checked_pow(u32 other);
    public NonZeroI16 saturating_pow(u32 other);
    public static NonZeroI16 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroI16 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroI16 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An [`i32`] that is known not to equal zero. */
@rust("std::num::NonZeroI32")
public class NonZeroI32 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroI32 isolate_highest_one();
    public NonZeroI32 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroI32 rotate_left(u32 n);
    public NonZeroI32 rotate_right(u32 n);
    public NonZeroI32 swap_bytes();
    public NonZeroI32 reverse_bits();
    public static NonZeroI32 from_be(NonZeroI32 x);
    public static NonZeroI32 from_le(NonZeroI32 x);
    public NonZeroI32 to_be();
    public NonZeroI32 to_le();
    public NonZeroI32 abs();
    public NonZeroI32? checked_abs();
    public (NonZeroI32, bool) overflowing_abs();
    public NonZeroI32 saturating_abs();
    public NonZeroI32 wrapping_abs();
    public NonZero<u32> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroI32? checked_neg();
    public (NonZeroI32, bool) overflowing_neg();
    public NonZeroI32 saturating_neg();
    public NonZeroI32 wrapping_neg();
    public NonZero<u32> cast_unsigned();
    public NonZeroI32? checked_mul(NonZeroI32 other);
    public NonZeroI32 saturating_mul(NonZeroI32 other);
    public unsafe NonZeroI32 unchecked_mul(NonZeroI32 other);
    public NonZeroI32? checked_pow(u32 other);
    public NonZeroI32 saturating_pow(u32 other);
    public static NonZeroI32 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroI32 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroI32 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An [`i64`] that is known not to equal zero. */
@rust("std::num::NonZeroI64")
public class NonZeroI64 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroI64 isolate_highest_one();
    public NonZeroI64 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroI64 rotate_left(u32 n);
    public NonZeroI64 rotate_right(u32 n);
    public NonZeroI64 swap_bytes();
    public NonZeroI64 reverse_bits();
    public static NonZeroI64 from_be(NonZeroI64 x);
    public static NonZeroI64 from_le(NonZeroI64 x);
    public NonZeroI64 to_be();
    public NonZeroI64 to_le();
    public NonZeroI64 abs();
    public NonZeroI64? checked_abs();
    public (NonZeroI64, bool) overflowing_abs();
    public NonZeroI64 saturating_abs();
    public NonZeroI64 wrapping_abs();
    public NonZero<ulong> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroI64? checked_neg();
    public (NonZeroI64, bool) overflowing_neg();
    public NonZeroI64 saturating_neg();
    public NonZeroI64 wrapping_neg();
    public NonZero<ulong> cast_unsigned();
    public NonZeroI64? checked_mul(NonZeroI64 other);
    public NonZeroI64 saturating_mul(NonZeroI64 other);
    public unsafe NonZeroI64 unchecked_mul(NonZeroI64 other);
    public NonZeroI64? checked_pow(u32 other);
    public NonZeroI64 saturating_pow(u32 other);
    public static NonZeroI64 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroI64 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroI64 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An [`i8`] that is known not to equal zero. */
@rust("std::num::NonZeroI8")
public class NonZeroI8 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroI8 isolate_highest_one();
    public NonZeroI8 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroI8 rotate_left(u32 n);
    public NonZeroI8 rotate_right(u32 n);
    public NonZeroI8 swap_bytes();
    public NonZeroI8 reverse_bits();
    public static NonZeroI8 from_be(NonZeroI8 x);
    public static NonZeroI8 from_le(NonZeroI8 x);
    public NonZeroI8 to_be();
    public NonZeroI8 to_le();
    public NonZeroI8 abs();
    public NonZeroI8? checked_abs();
    public (NonZeroI8, bool) overflowing_abs();
    public NonZeroI8 saturating_abs();
    public NonZeroI8 wrapping_abs();
    public NonZero<ubyte> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroI8? checked_neg();
    public (NonZeroI8, bool) overflowing_neg();
    public NonZeroI8 saturating_neg();
    public NonZeroI8 wrapping_neg();
    public NonZero<ubyte> cast_unsigned();
    public NonZeroI8? checked_mul(NonZeroI8 other);
    public NonZeroI8 saturating_mul(NonZeroI8 other);
    public unsafe NonZeroI8 unchecked_mul(NonZeroI8 other);
    public NonZeroI8? checked_pow(u32 other);
    public NonZeroI8 saturating_pow(u32 other);
    public static NonZeroI8 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroI8 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroI8 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An [`isize`] that is known not to equal zero. */
@rust("std::num::NonZeroIsize")
public class NonZeroIsize {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroIsize isolate_highest_one();
    public NonZeroIsize isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroIsize rotate_left(u32 n);
    public NonZeroIsize rotate_right(u32 n);
    public NonZeroIsize swap_bytes();
    public NonZeroIsize reverse_bits();
    public static NonZeroIsize from_be(NonZeroIsize x);
    public static NonZeroIsize from_le(NonZeroIsize x);
    public NonZeroIsize to_be();
    public NonZeroIsize to_le();
    public NonZeroIsize abs();
    public NonZeroIsize? checked_abs();
    public (NonZeroIsize, bool) overflowing_abs();
    public NonZeroIsize saturating_abs();
    public NonZeroIsize wrapping_abs();
    public NonZero<uint> unsigned_abs();
    public bool is_positive();
    public bool is_negative();
    public NonZeroIsize? checked_neg();
    public (NonZeroIsize, bool) overflowing_neg();
    public NonZeroIsize saturating_neg();
    public NonZeroIsize wrapping_neg();
    public NonZero<uint> cast_unsigned();
    public NonZeroIsize? checked_mul(NonZeroIsize other);
    public NonZeroIsize saturating_mul(NonZeroIsize other);
    public unsafe NonZeroIsize unchecked_mul(NonZeroIsize other);
    public NonZeroIsize? checked_pow(u32 other);
    public NonZeroIsize saturating_pow(u32 other);
    public static NonZeroIsize from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroIsize from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroIsize from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** A [`u128`] that is known not to equal zero. */
@rust("std::num::NonZeroU128")
public class NonZeroU128 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroU128 isolate_highest_one();
    public NonZeroU128 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroU128 rotate_left(u32 n);
    public NonZeroU128 rotate_right(u32 n);
    public NonZeroU128 swap_bytes();
    public NonZeroU128 reverse_bits();
    public static NonZeroU128 from_be(NonZeroU128 x);
    public static NonZeroU128 from_le(NonZeroU128 x);
    public NonZeroU128 to_be();
    public NonZeroU128 to_le();
    public NonZeroU128? checked_add(u128 other);
    public NonZeroU128 saturating_add(u128 other);
    public unsafe NonZeroU128 unchecked_add(u128 other);
    public NonZeroU128? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU128 midpoint(NonZeroU128 rhs);
    public bool is_power_of_two();
    public NonZeroU128 isqrt();
    public NonZero<i128> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroU128? checked_mul(NonZeroU128 other);
    public NonZeroU128 saturating_mul(NonZeroU128 other);
    public unsafe NonZeroU128 unchecked_mul(NonZeroU128 other);
    public NonZeroU128? checked_pow(u32 other);
    public NonZeroU128 saturating_pow(u32 other);
    public static NonZeroU128 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroU128 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroU128 from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZeroU128 div_ceil(NonZeroU128 rhs);
}

/** A [`u16`] that is known not to equal zero. */
@rust("std::num::NonZeroU16")
public class NonZeroU16 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroU16 isolate_highest_one();
    public NonZeroU16 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroU16 rotate_left(u32 n);
    public NonZeroU16 rotate_right(u32 n);
    public NonZeroU16 swap_bytes();
    public NonZeroU16 reverse_bits();
    public static NonZeroU16 from_be(NonZeroU16 x);
    public static NonZeroU16 from_le(NonZeroU16 x);
    public NonZeroU16 to_be();
    public NonZeroU16 to_le();
    public NonZeroU16? checked_add(ushort other);
    public NonZeroU16 saturating_add(ushort other);
    public unsafe NonZeroU16 unchecked_add(ushort other);
    public NonZeroU16? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU16 midpoint(NonZeroU16 rhs);
    public bool is_power_of_two();
    public NonZeroU16 isqrt();
    public NonZero<short> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroU16? checked_mul(NonZeroU16 other);
    public NonZeroU16 saturating_mul(NonZeroU16 other);
    public unsafe NonZeroU16 unchecked_mul(NonZeroU16 other);
    public NonZeroU16? checked_pow(u32 other);
    public NonZeroU16 saturating_pow(u32 other);
    public static NonZeroU16 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroU16 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroU16 from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZeroU16 div_ceil(NonZeroU16 rhs);
}

/** A [`u32`] that is known not to equal zero. */
@rust("std::num::NonZeroU32")
public class NonZeroU32 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroU32 isolate_highest_one();
    public NonZeroU32 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroU32 rotate_left(u32 n);
    public NonZeroU32 rotate_right(u32 n);
    public NonZeroU32 swap_bytes();
    public NonZeroU32 reverse_bits();
    public static NonZeroU32 from_be(NonZeroU32 x);
    public static NonZeroU32 from_le(NonZeroU32 x);
    public NonZeroU32 to_be();
    public NonZeroU32 to_le();
    public NonZeroU32? checked_add(u32 other);
    public NonZeroU32 saturating_add(u32 other);
    public unsafe NonZeroU32 unchecked_add(u32 other);
    public NonZeroU32? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU32 midpoint(NonZeroU32 rhs);
    public bool is_power_of_two();
    public NonZeroU32 isqrt();
    public NonZero<i32> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroU32? checked_mul(NonZeroU32 other);
    public NonZeroU32 saturating_mul(NonZeroU32 other);
    public unsafe NonZeroU32 unchecked_mul(NonZeroU32 other);
    public NonZeroU32? checked_pow(u32 other);
    public NonZeroU32 saturating_pow(u32 other);
    public static NonZeroU32 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroU32 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroU32 from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZeroU32 div_ceil(NonZeroU32 rhs);
}

/** A [`u64`] that is known not to equal zero. */
@rust("std::num::NonZeroU64")
public class NonZeroU64 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroU64 isolate_highest_one();
    public NonZeroU64 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroU64 rotate_left(u32 n);
    public NonZeroU64 rotate_right(u32 n);
    public NonZeroU64 swap_bytes();
    public NonZeroU64 reverse_bits();
    public static NonZeroU64 from_be(NonZeroU64 x);
    public static NonZeroU64 from_le(NonZeroU64 x);
    public NonZeroU64 to_be();
    public NonZeroU64 to_le();
    public NonZeroU64? checked_add(ulong other);
    public NonZeroU64 saturating_add(ulong other);
    public unsafe NonZeroU64 unchecked_add(ulong other);
    public NonZeroU64? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU64 midpoint(NonZeroU64 rhs);
    public bool is_power_of_two();
    public NonZeroU64 isqrt();
    public NonZero<long> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroU64? checked_mul(NonZeroU64 other);
    public NonZeroU64 saturating_mul(NonZeroU64 other);
    public unsafe NonZeroU64 unchecked_mul(NonZeroU64 other);
    public NonZeroU64? checked_pow(u32 other);
    public NonZeroU64 saturating_pow(u32 other);
    public static NonZeroU64 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroU64 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroU64 from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZeroU64 div_ceil(NonZeroU64 rhs);
}

/** A [`u8`] that is known not to equal zero. */
@rust("std::num::NonZeroU8")
public class NonZeroU8 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroU8 isolate_highest_one();
    public NonZeroU8 isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroU8 rotate_left(u32 n);
    public NonZeroU8 rotate_right(u32 n);
    public NonZeroU8 swap_bytes();
    public NonZeroU8 reverse_bits();
    public static NonZeroU8 from_be(NonZeroU8 x);
    public static NonZeroU8 from_le(NonZeroU8 x);
    public NonZeroU8 to_be();
    public NonZeroU8 to_le();
    public NonZeroU8? checked_add(ubyte other);
    public NonZeroU8 saturating_add(ubyte other);
    public unsafe NonZeroU8 unchecked_add(ubyte other);
    public NonZeroU8? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU8 midpoint(NonZeroU8 rhs);
    public bool is_power_of_two();
    public NonZeroU8 isqrt();
    public NonZero<byte> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroU8? checked_mul(NonZeroU8 other);
    public NonZeroU8 saturating_mul(NonZeroU8 other);
    public unsafe NonZeroU8 unchecked_mul(NonZeroU8 other);
    public NonZeroU8? checked_pow(u32 other);
    public NonZeroU8 saturating_pow(u32 other);
    public static NonZeroU8 from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroU8 from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroU8 from_str_radix(&String src, u32 radix) throws ParseIntError;
    public NonZeroU8 div_ceil(NonZeroU8 rhs);
}

/** A [`usize`] that is known not to equal zero. */
@rust("std::num::NonZeroUsize")
public class NonZeroUsize {
    public NonZeroUsize div_ceil(NonZeroUsize rhs);
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZeroUsize isolate_highest_one();
    public NonZeroUsize isolate_lowest_one();
    public u32 highest_one();
    public u32 lowest_one();
    public NonZero<u32> count_ones();
    public NonZeroUsize rotate_left(u32 n);
    public NonZeroUsize rotate_right(u32 n);
    public NonZeroUsize swap_bytes();
    public NonZeroUsize reverse_bits();
    public static NonZeroUsize from_be(NonZeroUsize x);
    public static NonZeroUsize from_le(NonZeroUsize x);
    public NonZeroUsize to_be();
    public NonZeroUsize to_le();
    public NonZeroUsize? checked_add(uint other);
    public NonZeroUsize saturating_add(uint other);
    public unsafe NonZeroUsize unchecked_add(uint other);
    public NonZeroUsize? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroUsize midpoint(NonZeroUsize rhs);
    public bool is_power_of_two();
    public NonZeroUsize isqrt();
    public NonZero<int> cast_signed();
    public NonZero<u32> bit_width();
    public NonZeroUsize? checked_mul(NonZeroUsize other);
    public NonZeroUsize saturating_mul(NonZeroUsize other);
    public unsafe NonZeroUsize unchecked_mul(NonZeroUsize other);
    public NonZeroUsize? checked_pow(u32 other);
    public NonZeroUsize saturating_pow(u32 other);
    public static NonZeroUsize from_ascii(ubyte[] src) throws ParseIntError;
    public static NonZeroUsize from_ascii_radix(ubyte[] src, u32 radix) throws ParseIntError;
    public static NonZeroUsize from_str_radix(&String src, u32 radix) throws ParseIntError;
}

/** An error returned from [`Path::normalize_lexically`] if a `..` parent reference */
@rust("std::path::NormalizeError")
public class NormalizeError {
}

/** An error indicating that an interior nul byte was found. */
@rust("std::ffi::c_str::NulError")
@RustClone
public class NulError implements ToOwned, ToString {
    public uint nul_position();
    public Vec<ubyte> into_vec();
}

/** This is the error type used by [`HandleOrNull`] when attempting to convert */
@rust("std::os::windows::io::NullHandleError")
@RustClone
public class NullHandleError {
}

@rust("std::sync::ONCE_INIT")
public const Once ONCE_INIT;

@rust("std::env::consts::OS")
public const String OS;

@rust("std::sys::pal::windows::c::windows_sys::OVERLAPPED")
public struct OVERLAPPED {
    public uint Internal;
    public uint InternalHigh;
    public OVERLAPPED_0 Anonymous;
    public c_void* hEvent;
}

@rust("std::sys::pal::windows::c::windows_sys::OVERLAPPED_0_0")
public struct OVERLAPPED_0_0 {
    public u32 Offset;
    public u32 OffsetHigh;
}

/** A view into an occupied entry in a `BTreeMap`. */
@rust("std::collections::btree_map::OccupiedEntry")
public class OccupiedEntry<K, V, A> {
    @RustRefOut public K key();
    public (K, V) remove_entry();
    @RustRefOut public V get();
    @MutSelf @RustRefOut public V get_mut();
    @RustRefOut public V into_mut();
    @MutSelf public V insert(V value);
    public V remove();
}

/** The error returned by [`try_insert`](BTreeMap::try_insert) when the key already exists. */
@rust("std::collections::btree_map::OccupiedError")
public struct OccupiedError<K, V, A> {
    public OccupiedEntry<K, V, A> entry;
    public K key;
    public V value;
}

/** A low-level synchronization primitive for one-time global execution. */
@rust("std::sync::Once")
public class Once {
    public Once();
    public void call_once<F>(() -> void f);
    public void call_once_force<F>((OnceState) -> void f);
    public bool is_completed();
    public void wait();
    public void wait_force();
}

/** A synchronization primitive which can nominally be written to only once. */
@rust("std::sync::OnceLock")
@RustClone
public class OnceLock<T> {
    public OnceLock();
    @RustRefOut public T? get();
    @MutSelf @RustRefOut public T? get_mut();
    @RustRefOut public T wait();
    public void set(T value) throws T;
    @RustRefOut public T try_insert(T value) throws Error;
    @RustRefOut public T get_or_init<F>(() -> T f);
    @MutSelf @RustRefOut public T get_mut_or_init<F>(() -> T f);
    @RustRefOut public T get_or_try_init<F, E>(() -> Result<T, E> f) throws E;
    @MutSelf @RustRefOut public T get_mut_or_try_init<F, E>(() -> Result<T, E> f) throws E;
    public T? into_inner();
    @MutSelf public T? take();
}

/** State yielded to [`Once::call_once_force()`]’s closure parameter. The state */
@rust("std::sync::OnceState")
public class OnceState {
    public bool is_poisoned();
}

/** Options and flags which can be used to configure how a file is opened. */
@rust("std::fs::OpenOptions")
@RustClone
public class OpenOptions implements OpenOptionsExt, OpenOptionsExt2 {
    public OpenOptions();
    @MutSelf @RustRefOut public OpenOptions read(bool read);
    @MutSelf @RustRefOut public OpenOptions write(bool write);
    @MutSelf @RustRefOut public OpenOptions append(bool append);
    @MutSelf @RustRefOut public OpenOptions truncate(bool truncate);
    @MutSelf @RustRefOut public OpenOptions create(bool create);
    @MutSelf @RustRefOut public OpenOptions create_new(bool create_new);
    public File open<P>(P path) throws Error;
}

/** Unix-specific extensions to [`fs::OpenOptions`]. */
@rust("std::os::unix::fs::OpenOptionsExt")
public interface OpenOptionsExt {
    @MutSelf @RustRefOut public Self mode(u32 mode);
    @MutSelf @RustRefOut public Self custom_flags(i32 flags);
}

@rust("std::os::windows::fs::OpenOptionsExt2")
public interface OpenOptionsExt2 {
    @MutSelf @RustRefOut public Self freeze_last_access_time(bool freeze);
    @MutSelf @RustRefOut public Self freeze_last_write_time(bool freeze);
}

/** An `Ordering` is the result of a comparison between two values. */
@rust("std::cmp::Ordering")
@RustClone
public enum Ordering implements Any, Clone, CloneToUninit, Copy, Debug, Eq, Hash, Ord, StructuralPartialEq {
    Less = -1, Equal = 0, Greater = 1;

    public bool is_eq();
    public bool is_ne();
    public bool is_lt();
    public bool is_gt();
    public bool is_le();
    public bool is_ge();
    public Ordering reverse();
    public Ordering then(Ordering other);
    public Ordering then_with<F>(() -> Ordering f);
}

/** Borrowed reference to an OS string (see [`OsString`]). */
@rust("std::ffi::os_str::OsStr")
@RustClone
@RustOwnedAs("OsString")
public class OsStr implements OsStrExt {
    public OsStr(&S s);
    @RustDefault public OsStr();
    @RustRefOut public static unsafe OsStr from_encoded_bytes_unchecked(ubyte[] bytes);
    @RustRefOut public String? to_str();
    @RustBorrowsSelf public Cow<String> to_string_lossy();
    public OsString to_os_string();
    public bool is_empty();
    public uint len();
    public OsString into_os_string();
    public (OsStr, OsStr) split_at(uint mid);
    public (OsStr, OsStr)? split_at_checked(uint mid);
    @RustRefOut public ubyte[] as_encoded_bytes();
    @RustRefOut public OsStr slice_encoded_bytes<R>(R range);
    @MutSelf public void make_ascii_lowercase();
    @MutSelf public void make_ascii_uppercase();
    public OsString to_ascii_lowercase();
    public OsString to_ascii_uppercase();
    public bool is_ascii();
    public bool eq_ignore_ascii_case<S>(S other);
    @RustBorrowsSelf public Display display();
    @RustRefOut public OsStr as_os_str();
}

/** Platform-specific extensions to [`OsStr`]. */
@rust("std::os::unix::ffi::OsStrExt")
public interface OsStrExt {
    @RustRefOut public Self from_bytes(ubyte[] slice);
    @RustRefOut public ubyte[] as_bytes();
}

/** A type that can represent owned, mutable platform-native strings, but is */
@rust("std::ffi::os_str::OsString")
@RustClone
@RustCollection
public class OsString implements OsStringExt {
    public OsString();
    public static unsafe OsString from_encoded_bytes_unchecked(Vec<ubyte> bytes);
    @RustRefOut public OsStr as_os_str();
    public Vec<ubyte> into_encoded_bytes();
    public String into_string() throws OsString;
    @MutSelf public void push<T>(T s);
    public static OsString with_capacity(uint capacity);
    @MutSelf public void clear();
    public uint capacity();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    public OsStr into_boxed_os_str();
    @RustRefOut public OsStr leak();
    @MutSelf public void truncate(uint len);
    @RustRefOut public String? to_str();
    @RustBorrowsSelf public Cow<String> to_string_lossy();
    public OsString to_os_string();
    public bool is_empty();
    public uint len();
    public OsString into_os_string();
    public (OsStr, OsStr) split_at(uint mid);
    public (OsStr, OsStr)? split_at_checked(uint mid);
    @RustRefOut public ubyte[] as_encoded_bytes();
    @RustRefOut public Self slice_encoded_bytes<R>(R range);
    @MutSelf public void make_ascii_lowercase();
    @MutSelf public void make_ascii_uppercase();
    public OsString to_ascii_lowercase();
    public OsString to_ascii_uppercase();
    public bool is_ascii();
    public bool eq_ignore_ascii_case<S>(S other);
    @RustBorrowsSelf public Display display();
}

/** Platform-specific extensions to [`OsString`]. */
@rust("std::os::unix::ffi::OsStringExt")
public interface OsStringExt {
    public Self from_vec(Vec<ubyte> vec);
    public Vec<ubyte> into_vec();
}

/** The output of a finished process. */
@rust("std::process::Output")
@RustClone
public class Output {
    public ExitStatus status;
    public Vec<ubyte> stdout;
    public Vec<ubyte> stderr;
    public Output exit_ok() throws ExitStatusError;
}

/** An owned file descriptor. */
@rust("std::os::fd::OwnedFd")
public class OwnedFd implements AsFd, AsRawFd, FromRawFd, IntoRawFd, IsTerminal {
    public OwnedFd try_clone() throws Error;
}

/** An owned handle. */
@rust("std::os::windows::io::OwnedHandle")
public class OwnedHandle implements AsHandle, AsRawHandle, FromRawHandle, IntoRawHandle, IsTerminal {
    public OwnedHandle try_clone() throws Error;
}

/** An owned socket. */
@rust("std::os::windows::io::OwnedSocket")
public class OwnedSocket implements AsRawSocket, AsSocket, FromRawSocket, IntoRawSocket {
    public OwnedSocket try_clone() throws Error;
}

/** A struct providing information about a panic. */
@rust("std::panic::PanicHookInfo")
public class PanicHookInfo {
    @RustRefOut public Any payload();
    @RustRefOut public String? payload_as_str();
    @RustRefOut public Location? location();
    public bool can_unwind();
}

/** An error which can be returned when parsing a float. */
@rust("std::num::ParseFloatError")
@RustClone
public class ParseFloatError implements Any, Clone, CloneToUninit, Debug, Display, Eq, Error, StructuralPartialEq {
}

/** An error which can be returned when parsing an integer. */
@rust("std::num::ParseIntError")
@RustClone
public class ParseIntError implements Any, Clone, CloneToUninit, Debug, Display, Eq, Error, StructuralPartialEq {
    @RustRefOut public IntErrorKind kind();
}

/** A slice of a path (akin to [`str`]). */
@rust("std::path::Path")
@RustClone
@RustOwnedAs("PathBuf")
public class Path {
    public Path(&S s);
    @RustRefOut public OsStr as_os_str();
    @MutSelf @RustRefOut public OsStr as_mut_os_str();
    @RustRefOut public String? to_str();
    @RustBorrowsSelf public Cow<String> to_string_lossy();
    public PathBuf to_path_buf();
    public bool is_absolute();
    public bool is_relative();
    public bool has_root();
    @RustRefOut public Path? parent();
    @RustBorrowsSelf public Ancestors ancestors();
    @RustRefOut public OsStr? file_name();
    @RustRefOut public Path strip_prefix<P>(P base) throws StripPrefixError;
    @RustRefOut public Path trim_prefix<P>(P base);
    public bool starts_with<P>(P base);
    public bool ends_with<P>(P child);
    public bool is_empty();
    @RustRefOut public OsStr? file_stem();
    @RustRefOut public OsStr? file_prefix();
    @RustRefOut public OsStr? extension();
    public bool has_trailing_sep();
    @RustBorrowsSelf public Cow<Path> with_trailing_sep();
    @RustRefOut public Path trim_trailing_sep();
    public PathBuf join<P>(P path);
    public PathBuf with_file_name<S>(S file_name);
    public PathBuf with_extension<S>(S extension);
    public PathBuf with_added_extension<S>(S extension);
    @RustBorrowsSelf public Components components();
    @RustBorrowsSelf public Iter iter();
    @RustBorrowsSelf public Display display();
    @RustRefOut public Path as_path();
    public Metadata metadata() throws Error;
    public Metadata symlink_metadata() throws Error;
    public PathBuf canonicalize() throws Error;
    public PathBuf absolute() throws Error;
    public PathBuf normalize_lexically() throws NormalizeError;
    public PathBuf read_link() throws Error;
    public ReadDir read_dir() throws Error;
    public bool exists();
    public bool try_exists() throws Error;
    public bool is_file();
    public bool is_dir();
    public bool is_symlink();
    public PathBuf into_path_buf();
}

/** An owned, mutable path (akin to [`String`]). */
@rust("std::path::PathBuf")
@RustClone
@RustCollection
public class PathBuf {
    public PathBuf();
    public static PathBuf with_capacity(uint capacity);
    @RustRefOut public Path as_path();
    @RustRefOut public Path leak();
    @MutSelf public void push<P>(P path);
    @MutSelf public bool pop();
    @MutSelf public void set_trailing_sep(bool trailing_sep);
    @MutSelf public void push_trailing_sep();
    @MutSelf public void pop_trailing_sep();
    @MutSelf public void set_file_name<S>(S file_name);
    @MutSelf public bool set_extension<S>(S extension);
    @MutSelf public bool add_extension<S>(S extension);
    @MutSelf @RustRefOut public OsString as_mut_os_string();
    public OsString into_os_string();
    public String into_string() throws PathBuf;
    public Path into_boxed_path();
    public uint capacity();
    @MutSelf public void clear();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @RustRefOut public OsStr as_os_str();
    @MutSelf @RustRefOut public OsStr as_mut_os_str();
    @RustRefOut public String? to_str();
    @RustBorrowsSelf public Cow<String> to_string_lossy();
    public PathBuf to_path_buf();
    public bool is_absolute();
    public bool is_relative();
    public bool has_root();
    @RustRefOut public Path? parent();
    @RustBorrowsSelf public Ancestors ancestors();
    @RustRefOut public OsStr? file_name();
    @RustRefOut public Path strip_prefix<P>(P base) throws StripPrefixError;
    @RustRefOut public Path trim_prefix<P>(P base);
    public bool starts_with<P>(P base);
    public bool ends_with<P>(P child);
    public bool is_empty();
    @RustRefOut public OsStr? file_stem();
    @RustRefOut public OsStr? file_prefix();
    @RustRefOut public OsStr? extension();
    public bool has_trailing_sep();
    @RustBorrowsSelf public Cow<Path> with_trailing_sep();
    @RustRefOut public Path trim_trailing_sep();
    public PathBuf join<P>(P path);
    public PathBuf with_file_name<S>(S file_name);
    public PathBuf with_extension<S>(S extension);
    public PathBuf with_added_extension<S>(S extension);
    @RustBorrowsSelf public Components components();
    @RustBorrowsSelf public Iter iter();
    @RustBorrowsSelf public Display display();
    public Metadata metadata() throws Error;
    public Metadata symlink_metadata() throws Error;
    public PathBuf canonicalize() throws Error;
    public PathBuf absolute() throws Error;
    public PathBuf normalize_lexically() throws NormalizeError;
    public PathBuf read_link() throws Error;
    public ReadDir read_dir() throws Error;
    public bool exists();
    public bool try_exists() throws Error;
    public bool is_file();
    public bool is_dir();
    public bool is_symlink();
    public PathBuf into_path_buf();
}

/** Structure wrapping a mutable reference to the greatest item on a */
@rust("std::collections::PeekMut")
public class PeekMut<T, A> {
    @MutSelf public bool refresh();
    public static T pop(PeekMut<T, A> this);
}

/** Representation of the various permissions on a file. */
@rust("std::fs::Permissions")
@RustClone
public class Permissions implements PermissionsExt {
    public bool readonly();
    @MutSelf public void set_readonly(bool readonly);
}

/** Unix-specific extensions to [`fs::Permissions`]. */
@rust("std::os::unix::fs::PermissionsExt")
public interface PermissionsExt {
    public u32 mode();
    @MutSelf public void set_mode(u32 mode);
    public Self from_mode(u32 mode);
}

/** This type represents a file descriptor that refers to a process. */
@rust("std::os::linux::process::PidFd")
public class PidFd implements AsFd, AsRawFd, FromRawFd, IntoRawFd {
    public void kill() throws Error;
    public ExitStatus wait() throws Error;
    public ExitStatus? try_wait() throws Error;
}

/** A pointer which pins its pointee in place. */
@rust("std::pin::Pin")
@RustClone
public class Pin<Ptr> implements Any, AsyncIterator, Clone, CloneToUninit, Copy, Debug, Deref, DerefMut, DerefPure, Display, Eq, Future, Hash, IntoAsyncIterator, IntoFuture, Ord, PinCoerceUnsized, Pointer, Receiver {
    public Pin(Ptr pointer);
    public static Ptr into_inner(Pin<Ptr> pin);
    public static unsafe Pin<Ptr> new_unchecked(Ptr pointer);
    @RustRefOut public Pin<Ptr.Target> as_ref();
    @MutSelf @RustRefOut public Pin<Ptr.Target> as_mut();
    @RustRefOut public Pin<Ptr.Target> as_deref_mut();
    @MutSelf public void set(Ptr.Target value);
    public static unsafe Ptr into_inner_unchecked(Pin<Ptr> pin);
    @RustRefOut public unsafe Pin<U> map_unchecked<U, F>((T) -> U func);
    @RustRefOut public T get_ref();
    @RustRefOut public Pin<T> into_ref();
    @RustRefOut public T get_mut();
    @RustRefOut public unsafe T get_unchecked_mut();
    @RustRefOut public unsafe Pin<U> map_unchecked_mut<U, F>((T) -> U func);
    @RustRefOut public static Pin<T> static_ref(&T r);
    @RustRefOut public static Pin<T> static_mut(&mut T r);
}

/** Read end of an anonymous pipe. */
@rust("std::io::PipeReader")
public class PipeReader implements AsFd, AsHandle, AsRawFd, AsRawHandle, FromRawFd, FromRawHandle, IntoRawFd, IntoRawHandle, Read {
    public PipeReader try_clone() throws Error;
}

/** Write end of an anonymous pipe. */
@rust("std::io::PipeWriter")
public class PipeWriter implements AsFd, AsHandle, AsRawFd, AsRawHandle, FromRawFd, FromRawHandle, IntoRawFd, IntoRawHandle, Write {
    public PipeWriter try_clone() throws Error;
}

/** A type of error which can be returned whenever a lock is acquired. */
@rust("std::sync::poison::PoisonError")
public class PoisonError<T> {
    public PoisonError(T data);
    public T into_inner();
    @RustRefOut public T get_ref();
    @MutSelf @RustRefOut public T get_mut();
}

/** Windows path prefixes, e.g., `C:` or `\\server\share`. */
@rust("std::path::Prefix")
@RustClone
public enum Prefix {
    Verbatim(OsStr), VerbatimUNC(OsStr, OsStr), VerbatimDisk(ubyte), DeviceNS(OsStr), UNC(OsStr, OsStr), Disk(ubyte);

    public bool is_verbatim();
}

/** A structure wrapping a Windows path prefix as well as its unparsed string */
@rust("std::path::PrefixComponent")
@RustClone
public class PrefixComponent {
    @RustBorrowsSelf public Prefix kind();
    @RustRefOut public OsStr as_os_str();
}

/** A wrapper around windows [`ProcThreadAttributeList`][1]. */
@rust("std::os::windows::process::ProcThreadAttributeList")
public class ProcThreadAttributeList {
    public static ProcThreadAttributeListBuilder build();
}

/** Builder for constructing a [`ProcThreadAttributeList`]. */
@rust("std::os::windows::process::ProcThreadAttributeListBuilder")
@RustClone
public class ProcThreadAttributeListBuilder {
    public ProcThreadAttributeListBuilder attribute<T>(uint attribute, &T value);
    public unsafe ProcThreadAttributeListBuilder raw_attribute<T>(uint attribute, T* value_ptr, uint value_size);
    @RustBorrowsSelf public ProcThreadAttributeList finish() throws Error;
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::RChunks")
@RustClone
public class RChunks<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::RChunksExact")
@RustClone
public class RChunksExact<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @RustRefOut public T[] remainder();
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::RChunksExactMut")
public class RChunksExactMut<T> implements Any, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, Send, Sync, TrustedLen {
    @RustRefOut public T[] into_remainder();
    @MutSelf public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::RChunksMut")
public class RChunksMut<T> implements Any, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, Send, Sync, TrustedLen {
    @MutSelf public T[]? next();
}

/** Created with the method [`rmatch_indices`]. */
@rust("std::str::RMatchIndices")
@RustClone
public class RMatchIndices<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public (uint, String)? next();
}

/** Created with the method [`rmatches`]. */
@rust("std::str::RMatches")
@RustClone
public class RMatches<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public String? next();
}

/** An iterator over the subslices of the vector which are separated */
@rust("std::slice::RSplitMut")
public class RSplitMut<T, P> implements Any, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator over subslices separated by elements that match a */
@rust("std::slice::RSplitNMut")
public class RSplitNMut<T, P> implements Any, Debug, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** Created with the method [`rsplit_terminator`]. */
@rust("std::str::RSplitTerminator")
@RustClone
public class RSplitTerminator<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String? remainder();
    @MutSelf public String? next();
}

/** `RandomState` is the default state for [`HashMap`] types. */
@rust("std::hash::RandomState")
@RustClone
public class RandomState {
    public RandomState();
}

/** An iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::btree_map::Range")
@RustClone
public class Range<K, V> implements ToOwned {
    @RustDefault public Range();
    @MutSelf public (K, V)? next();
}

/** A mutable iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::btree_map::RangeMut")
public class RangeMut<K, V> {
    @RustDefault public RangeMut();
    @MutSelf public (K, V)? next();
}

public type RawPthread = pthread_t;


public type RawSocket = SOCKET;


/** A single-threaded reference-counting pointer. 'Rc' stands for 'Reference */
@rust("std::rc::Rc")
@RustClone
public class Rc<T, A> implements ToOwned, ToString {
    public Rc(T value);
    @RustDefault public Rc();
    public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static T try_new(T value) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static Pin<T> pin(T value);
    public static U map<U>(Rc this, (T) -> U f);
    public static Self.TryType try_map<R>(Rc this, (T) -> R f);
    public static T new_in(T value, A alloc);
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static T new_cyclic_in<F>((Weak<T, A>) -> T data_fn, A alloc);
    public static Rc try_new_in(T value, A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static Pin<Rc> pin_in(T value, A alloc);
    public static T try_unwrap(Rc this) throws Rc;
    public static T? into_inner(Rc this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public static MaybeUninit<T>[] new_uninit_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] new_zeroed_slice_in(uint len, A alloc);
    public T[] into_array() throws Rc;
    public unsafe T assume_init();
    public static T clone_from_ref(&T value);
    public static T try_clone_from_ref(&T value) throws AllocError;
    public static T clone_from_ref_in(&T value, A alloc);
    public static T try_clone_from_ref_in(&T value, A alloc) throws AllocError;
    public static unsafe Rc from_raw(T* ptr);
    public static T* into_raw(Rc this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    @RustRefOut public static A allocator(&Rc this);
    public static (T*, A) into_raw_with_allocator(Rc this);
    public static T* as_ptr(&Rc this);
    public static unsafe Rc from_raw_in(T* ptr, A alloc);
    public static Weak<T, A> downgrade(&Rc this);
    public static uint weak_count(&Rc this);
    public static uint strong_count(&Rc this);
    public static unsafe void increment_strong_count_in(T* ptr, A alloc);
    public static unsafe void decrement_strong_count_in(T* ptr, A alloc);
    @RustRefOut public static T? get_mut(&mut Rc this);
    @RustRefOut public static unsafe T get_mut_unchecked(&mut Rc this);
    public static bool ptr_eq(&Rc this, &Rc other);
    @RustRefOut public static T make_mut(&mut Rc this);
    public static T unwrap_or_clone(Rc this);
    public T downcast<T>() throws Rc;
    public unsafe T downcast_unchecked<T>();
}

/** The `Read` trait allows for reading bytes from a source. */
@rust("std::io::Read")
public interface Read {
    @MutSelf public uint read(&mut ubyte[] buf) throws Error;
    @MutSelf public uint read_vectored(&mut IoSliceMut[] bufs) throws Error;
    public bool is_read_vectored();
    @MutSelf public uint read_to_end(&mut Vec<ubyte> buf) throws Error;
    @MutSelf public uint read_to_string(&mut String buf) throws Error;
    @MutSelf public void read_exact(&mut ubyte[] buf) throws Error;
    @MutSelf public void read_buf(BorrowedCursor<ubyte> buf) throws Error;
    @MutSelf public void read_buf_exact(BorrowedCursor<ubyte> cursor) throws Error;
    @MutSelf @RustRefOut public Self by_ref();
    public Bytes<Self> bytes();
    public Chain<Self, R> chain<R>(R next);
    public Take<Self> take(ulong limit);
    @MutSelf public ubyte[] read_array() throws Error;
    @MutSelf public T read_le<T>() throws Error;
    @MutSelf public T read_be<T>() throws Error;
}

/** Iterator over the entries in a directory. */
@rust("std::fs::ReadDir")
public class ReadDir {
    @MutSelf public DirEntry? next() throws Error;
}

/** The receiving half of Rust's [`channel`] (or [`sync_channel`]) type. */
@rust("std::sync::mpmc::Receiver")
@RustClone
public class Receiver<T> {
    public T try_recv() throws TryRecvError;
    public T recv() throws RecvError;
    public T recv_timeout(Duration timeout) throws RecvTimeoutError;
    public T recv_deadline(Instant deadline) throws RecvTimeoutError;
    @RustBorrowsSelf public TryIter<T> try_iter();
    public bool is_empty();
    public bool is_full();
    public uint len();
    public uint? capacity();
    public bool same_channel(&Receiver<T> other);
    @RustBorrowsSelf public Iter<T> iter();
    public bool is_disconnected();
}

/** An error returned from the [`recv`] function on a [`Receiver`]. */
@rust("std::sync::mpsc::RecvError")
@RustClone
public class RecvError {
}

/** This enumeration is the list of possible errors that made [`recv_timeout`] */
@rust("std::sync::mpsc::RecvTimeoutError")
@RustClone
public enum RecvTimeoutError {
    Timeout, Disconnected
}

/** A re-entrant mutual exclusion lock */
@rust("std::sync::ReentrantLock")
public class ReentrantLock<T> {
    public ReentrantLock(T t);
    @RustDefault public ReentrantLock();
    public T into_inner();
    @RustBorrowsSelf public ReentrantLockGuard<T> lock();
    @MutSelf @RustRefOut public T get_mut();
    public T* data_ptr();
}

/** An RAII implementation of a "scoped lock" of a re-entrant lock. When this */
@rust("std::sync::ReentrantLockGuard")
public class ReentrantLockGuard<T> {
}

/** A reader which yields one byte over and over and over and over and over and... */
@rust("std::io::Repeat")
public class Repeat implements Any, Debug {
}

/** An error reporter that prints an error and its sources. */
@rust("std::error::Report")
public class Report<E> {
    public Report(E error);
    public Report pretty(bool pretty);
    public Report show_backtrace(bool show_backtrace);
}

/** `Request` supports generic, type-driven access to data. Its use is currently restricted to the */
@rust("std::error::Request")
public class Request implements Any, Debug {
    @MutSelf @RustRefOut public Request provide_value<T>(T value);
    @MutSelf @RustRefOut public Request provide_value_with<T>(() -> T fulfil);
    @MutSelf @RustRefOut public Request provide_ref<T>(&T value);
    @MutSelf @RustRefOut public Request provide_ref_with<T>(() -> T fulfil);
    public bool would_be_satisfied_by_value_of<T>();
    public bool would_be_satisfied_by_ref_of<T>();
}

/** A reader-writer lock */
@rust("std::sync::RwLock")
public class RwLock<T> {
    public RwLock(T t);
    @RustDefault public RwLock();
    public T get_cloned() throws PoisonError<void>;
    public void set(T value) throws PoisonError<T>;
    public T replace(T value) throws PoisonError<T>;
    @RustBorrowsSelf public RwLockReadGuard<T> read() throws PoisonError<T>;
    @RustBorrowsSelf public RwLockReadGuard<T> try_read() throws TryLockError<Guard>;
    @RustBorrowsSelf public RwLockWriteGuard<T> write() throws PoisonError<T>;
    @RustBorrowsSelf public RwLockWriteGuard<T> try_write() throws TryLockError<Guard>;
    public bool is_poisoned();
    public void clear_poison();
    public T into_inner() throws PoisonError<T>;
    @MutSelf @RustRefOut public T get_mut() throws PoisonError<T>;
    public T* data_ptr();
}

/** RAII structure used to release the shared read access of a lock when */
@rust("std::sync::RwLockReadGuard")
public class RwLockReadGuard<T> {
    public static MappedRwLockReadGuard<U> map<U, F>(RwLockReadGuard orig, (T) -> U f);
    public static MappedRwLockReadGuard<U> filter_map<U, F>(RwLockReadGuard orig, (T) -> U? f) throws RwLockReadGuard;
}

/** RAII structure used to release the exclusive write access of a lock when */
@rust("std::sync::RwLockWriteGuard")
public class RwLockWriteGuard<T> {
    public static RwLockReadGuard<T> downgrade(RwLockWriteGuard s);
    public static MappedRwLockWriteGuard<U> map<U, F>(RwLockWriteGuard orig, (T) -> U f);
    public static MappedRwLockWriteGuard<U> filter_map<U, F>(RwLockWriteGuard orig, (T) -> U? f) throws RwLockWriteGuard;
}

@rust("std::path::SEPARATORS")
public const char[] SEPARATORS;

@rust("std::path::SEPARATORS_STR")
public const String[] SEPARATORS_STR;

@rust("std::sys::pal::windows::c::windows_sys::SOCKADDR")
public struct SOCKADDR {
    public ushort sa_family;
    public byte[14] sa_data;
}

public type SOCKET = ulong;


@rust("std::os::fd::stdio::STDERR")
public const BorrowedFd STDERR;

@rust("std::os::fd::stdio::STDIN")
public const BorrowedFd STDIN;

@rust("std::os::fd::stdio::STDOUT")
public const BorrowedFd STDOUT;

/** Provides intentionally-saturating arithmetic on `T`. */
@rust("std::num::Saturating")
@RustClone
public class Saturating<T> implements Any, Binary, Clone, CloneToUninit, Copy, Debug, Default, Display, Eq, Hash, LowerHex, Octal, Ord, StructuralPartialEq, UpperHex {
    @RustDefault public Saturating();
    public u32 count_ones();
    public u32 count_zeros();
    public u32 trailing_zeros();
    public Saturating rotate_left(u32 n);
    public Saturating rotate_right(u32 n);
    public Saturating swap_bytes();
    public Saturating reverse_bits();
    public static Saturating from_be(Saturating x);
    public static Saturating from_le(Saturating x);
    public Saturating to_be();
    public Saturating to_le();
    public Saturating pow(u32 exp);
    public u32 leading_zeros();
    public Saturating<int> abs();
    public Saturating<int> signum();
    public bool is_positive();
    public bool is_negative();
    public bool is_power_of_two();
}

@rust("std::os::unix::net::ScmCredentials")
public class ScmCredentials {
    @MutSelf public SocketCred? next();
}

/** This control message contains file descriptors. */
@rust("std::os::unix::net::ScmRights")
public class ScmRights {
    @MutSelf public i32? next();
}

/** A scope to spawn scoped threads in. */
@rust("std::thread::Scope")
public class Scope {
    @RustBorrowsSelf public ScopedJoinHandle<T> spawn<F, T>(() -> T f);
}

/** An owned permission to join on a scoped thread (block on its termination). */
@rust("std::thread::ScopedJoinHandle")
public class ScopedJoinHandle<T> {
    @RustRefOut public Thread thread();
    public T join() throws Error;
    public bool is_finished();
}

/** This trait being unreachable from outside the crate */
@rust("std::sealed::Sealed")
public interface Sealed {
}

/** The `Seek` trait provides a cursor which can be moved within a stream of */
@rust("std::io::Seek")
public interface Seek {
    @MutSelf public ulong seek(SeekFrom pos) throws Error;
    @MutSelf public void rewind() throws Error;
    @MutSelf public ulong stream_len() throws Error;
    @MutSelf public ulong stream_position() throws Error;
    @MutSelf public void seek_relative(long offset) throws Error;
}

/** Enumeration of possible methods to seek within an I/O object. */
@rust("std::io::SeekFrom")
@RustClone
public enum SeekFrom {
    Start(ulong), End(long), Current(long)
}

/** An error returned from the [`Sender::send`] or [`SyncSender::send`] */
@rust("std::sync::mpsc::SendError")
@RustClone
public class SendError<T> {
}

/** An error returned from the [`send_timeout`] method. */
@rust("std::sync::mpmc::SendTimeoutError")
@RustClone
public enum SendTimeoutError<T> {
    Timeout(T), Disconnected(T)
}

/** The sending-half of Rust's synchronous [`channel`] type. */
@rust("std::sync::mpmc::Sender")
@RustClone
public class Sender<T> {
    public void try_send(T msg) throws TrySendError<T>;
    public void send(T msg) throws SendError<T>;
    public void send_timeout(T msg, Duration timeout) throws SendTimeoutError<T>;
    public void send_deadline(T msg, Instant deadline) throws SendTimeoutError<T>;
    public bool is_empty();
    public bool is_full();
    public uint len();
    public uint? capacity();
    public bool same_channel(&Sender<T> other);
    public bool is_disconnected();
}

/** Possible values which can be passed to the [`TcpStream::shutdown`] method. */
@rust("std::net::Shutdown")
@RustClone
public enum Shutdown {
    Read, Write, Both
}

/** A SIMD vector with the shape of `[T; N]` but the operations of `T`. */
@rust("std::simd::Simd")
@RustClone
public class Simd<T> implements Any, Clone, CloneToUninit, Copy, Debug, Default, Eq, Hash, Ord {
    @RustDefault public Simd();
    public Simd reverse();
    public Simd rotate_elements_left();
    public Simd rotate_elements_right();
    public Simd shift_elements_left(T padding);
    public Simd shift_elements_right(T padding);
    public (Simd, Simd) interleave(Simd other);
    public (Simd, Simd) deinterleave(Simd other);
    public Simd<T> resize(T value);
    public Simd<T> extract();
    public Simd swizzle_dyn(Simd<ubyte> idxs);
    public uint len();
    public static Simd splat(T value);
    @RustRefOut public T[] as_array();
    @MutSelf @RustRefOut public T[] as_mut_array();
    public static Simd from_array(T[] array);
    public T[] to_array();
    public static Simd from_slice(T[] slice);
    public void copy_to_slice(&mut T[] slice);
    public static Simd load_or_default(T[] slice);
    public static Simd load_or(T[] slice, Simd or);
    public static Simd load_select_or_default(T[] slice, Mask<T.Mask> enable);
    public static Simd load_select(T[] slice, Mask<T.Mask> enable, Simd or);
    public static unsafe Simd load_select_unchecked(T[] slice, Mask<T.Mask> enable, Simd or);
    public static unsafe Simd load_select_ptr(T* ptr, Mask<T.Mask> enable, Simd or);
    public static Simd gather_or(T[] slice, Simd<uint> idxs, Simd or);
    public static Simd gather_or_default(T[] slice, Simd<uint> idxs);
    public static Simd gather_select(T[] slice, Mask<int> enable, Simd<uint> idxs, Simd or);
    public static unsafe Simd gather_select_unchecked(T[] slice, Mask<int> enable, Simd<uint> idxs, Simd or);
    public static unsafe Simd gather_ptr(Simd<T*> source);
    public static unsafe Simd gather_select_ptr(Simd<T*> source, Mask<int> enable, Simd or);
    public void store_select(&mut T[] slice, Mask<T.Mask> enable);
    public unsafe void store_select_unchecked(&mut T[] slice, Mask<T.Mask> enable);
    public unsafe void store_select_ptr(T* ptr, Mask<T.Mask> enable);
    public void scatter(&mut T[] slice, Simd<uint> idxs);
    public void scatter_select(&mut T[] slice, Mask<int> enable, Simd<uint> idxs);
    public unsafe void scatter_select_unchecked(&mut T[] slice, Mask<int> enable, Simd<uint> idxs);
    public unsafe void scatter_ptr(Simd<T*> dest);
    public unsafe void scatter_select_ptr(Simd<T*> dest, Mask<int> enable);
}

/** A writer which will move data into the void. */
@rust("std::io::Sink")
@RustClone
public class Sink implements Any, Clone, CloneToUninit, Copy, Debug, Default {
    @RustDefault public Sink();
}

@rust("std::sys::net::connection::socket::windows::Socket")
public class Socket {
}

/** An address associated with a Unix socket. */
@rust("std::os::unix::net::SocketAddr")
@RustClone
public class SocketAddr implements SocketAddrExt {
    public static SocketAddr from_pathname<P>(P path) throws Error;
    public bool is_unnamed();
    @RustRefOut public Path? as_pathname();
}

/** Platform-specific extensions to [`SocketAddr`]. */
@rust("std::os::linux::net::SocketAddrExt")
public interface SocketAddrExt {
    public SocketAddr from_abstract_name<N>(N name) throws Error;
    @RustRefOut public ubyte[]? as_abstract_name();
}

/** An IPv4 socket address. */
@rust("std::net::SocketAddrV4")
@RustClone
public class SocketAddrV4 implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, FromStr, Hash, Ord, StructuralPartialEq {
    public SocketAddrV4(Ipv4Addr ip, ushort port);
    public static SocketAddrV4 parse_ascii(ubyte[] b) throws AddrParseError;
    @RustRefOut public Ipv4Addr ip();
    @MutSelf public void set_ip(Ipv4Addr new_ip);
    public ushort port();
    @MutSelf public void set_port(ushort new_port);
}

/** An IPv6 socket address. */
@rust("std::net::SocketAddrV6")
@RustClone
public class SocketAddrV6 implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, FromStr, Hash, Ord, StructuralPartialEq {
    public SocketAddrV6(Ipv6Addr ip, ushort port, u32 flowinfo, u32 scope_id);
    public static SocketAddrV6 parse_ascii(ubyte[] b) throws AddrParseError;
    @RustRefOut public Ipv6Addr ip();
    @MutSelf public void set_ip(Ipv6Addr new_ip);
    public ushort port();
    @MutSelf public void set_port(ushort new_port);
    public u32 flowinfo();
    @MutSelf public void set_flowinfo(u32 new_flowinfo);
    public u32 scope_id();
    @MutSelf public void set_scope_id(u32 new_scope_id);
}

/** A Unix socket Ancillary data struct. */
@rust("std::os::unix::net::SocketAncillary")
public class SocketAncillary {
    public SocketAncillary(&mut ubyte[] buffer);
    public uint capacity();
    public bool is_empty();
    public uint len();
    @RustBorrowsSelf public Messages messages();
    public bool truncated();
    @MutSelf public bool add_fds(RawFd[] fds);
    @MutSelf public bool add_creds(SocketCred[] creds);
    @MutSelf public void clear();
}

@rust("std::os::unix::net::SocketCred")
@RustClone
public class SocketCred {
}

/** A splicing iterator for `Vec`. */
@rust("std::vec::Splice")
public class Splice<I, A> {
    @MutSelf public I.Item? next();
}

/** An iterator over the contents of an instance of `BufRead` split on a */
@rust("std::io::Split")
public class Split<B> {
    @MutSelf public Vec<ubyte>? next() throws Error;
}

/** An iterator over the non-ASCII-whitespace substrings of a string, */
@rust("std::str::SplitAsciiWhitespace")
@RustClone
public class SplitAsciiWhitespace implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String? remainder();
    @MutSelf public String? next();
}

/** An iterator over the mutable subslices of the vector which are separated */
@rust("std::slice::SplitInclusiveMut")
public class SplitInclusiveMut<T, P> implements Any, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator over the mutable subslices of the vector which are separated */
@rust("std::slice::SplitMut")
public class SplitMut<T, P> implements Any, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator over subslices separated by elements that match a predicate */
@rust("std::slice::SplitNMut")
public class SplitNMut<T, P> implements Any, Debug, FusedIterator, IntoIterator, Iterator {
    @MutSelf public T[]? next();
}

/** An iterator that splits an environment variable into paths according to */
@rust("std::env::SplitPaths")
public class SplitPaths {
    @MutSelf public PathBuf? next();
}

/** Created with the method [`split_terminator`]. */
@rust("std::str::SplitTerminator")
@RustClone
public class SplitTerminator<P> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String? remainder();
    @MutSelf public String? next();
}

/** An iterator over the non-whitespace substrings of a string, */
@rust("std::str::SplitWhitespace")
@RustClone
public class SplitWhitespace implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, FusedIterator, IntoIterator, Iterator {
    @RustRefOut public String? remainder();
    @MutSelf public String? next();
}

/** This trait provides a possibly-temporary implementation of float functions */
@rust("std::simd::StdFloat")
public interface StdFloat {
    public Self mul_add(Self a, Self b);
    public Self sqrt();
    public Self sin();
    public Self cos();
    public Self exp();
    public Self exp2();
    public Self ln();
    public Self log(Self base);
    public Self log2();
    public Self log10();
    public Self ceil();
    public Self floor();
    public Self round();
    public Self trunc();
    public Self round_ties_even();
    public Self fract();
}

/** A handle to the standard error stream of a process. */
@rust("std::io::Stderr")
public class Stderr implements AsFd, AsHandle, AsRawFd, AsRawHandle, IsTerminal, StdioExt, Write {
    public StderrLock lock();
}

/** A locked reference to the [`Stderr`] handle. */
@rust("std::io::StderrLock")
public class StderrLock implements AsFd, AsHandle, AsRawFd, AsRawHandle, IsTerminal, StdioExt, Write {
}

/** A handle to the standard input stream of a process. */
@rust("std::io::Stdin")
public class Stdin implements AsFd, AsHandle, AsRawFd, AsRawHandle, IsTerminal, Read, StdioExt {
    public StdinLock lock();
    public uint read_line(&mut String buf) throws Error;
    public Lines<StdinLock> lines();
}

/** A locked reference to the [`Stdin`] handle. */
@rust("std::io::StdinLock")
public class StdinLock implements AsFd, AsHandle, AsRawFd, AsRawHandle, BufRead, IsTerminal, Read, StdioExt {
}

/** Describes what to do with a standard I/O stream for a child process when */
@rust("std::process::Stdio")
public class Stdio implements FromRawFd, FromRawHandle {
    public static Stdio piped();
    public static Stdio inherit();
    public static Stdio null();
    public bool makes_pipe();
}

@rust("std::os::unix::io::StdioExt")
public interface StdioExt {
    @MutSelf public void set_fd<T>(T fd) throws Error;
    @MutSelf public OwnedFd replace_fd<T>(T replace_with) throws Error;
    @MutSelf public OwnedFd take_fd() throws Error;
}

/** A handle to the global standard output stream of the current process. */
@rust("std::io::Stdout")
public class Stdout implements AsFd, AsHandle, AsRawFd, AsRawHandle, IsTerminal, StdioExt, Write {
    public StdoutLock lock();
}

/** A locked reference to the [`Stdout`] handle. */
@rust("std::io::StdoutLock")
public class StdoutLock implements AsFd, AsHandle, AsRawFd, AsRawHandle, IsTerminal, StdioExt, Write {
}

/** A UTF-8–encoded, growable string. */
@rust("std::string::String")
@RustClone
@RustCollection
public class String implements ToOwned, ToString {
    public String();
    public static String with_capacity(uint capacity);
    public static String try_with_capacity(uint capacity) throws TryReserveError;
    public static String from_utf8(Vec<ubyte> vec) throws FromUtf8Error;
    public static Cow<String> from_utf8_lossy(ubyte[] v);
    public static String from_utf8_lossy_owned(Vec<ubyte> v);
    public static String from_utf16(ushort[] v) throws FromUtf16Error;
    public static String from_utf16_lossy(ushort[] v);
    public static String from_utf16le(ubyte[] v) throws FromUtf16Error;
    public static String from_utf16le_lossy(ubyte[] v);
    public static String from_utf16be(ubyte[] v) throws FromUtf16Error;
    public static String from_utf16be_lossy(ubyte[] v);
    public (ubyte*, uint, uint) into_raw_parts();
    public static unsafe String from_raw_parts(ubyte* buf, uint length, uint capacity);
    public static unsafe String from_utf8_unchecked(Vec<ubyte> bytes);
    public Vec<ubyte> into_bytes();
    @RustRefOut public String as_str();
    @MutSelf @RustRefOut public String as_mut_str();
    @MutSelf public void push_str(&String string);
    @MutSelf public void extend_from_within<R>(R src);
    public uint capacity();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf public void push(char ch);
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf public void truncate(uint new_len);
    @MutSelf public char? pop();
    @MutSelf public char remove(uint idx);
    @MutSelf public void remove_matches<P>(P pat);
    @MutSelf public void retain<F>((char) -> bool f);
    @MutSelf public void insert(uint idx, char ch);
    @MutSelf public void insert_str(uint idx, &String string);
    @MutSelf @RustRefOut public unsafe Vec<ubyte> as_mut_vec();
    public uint len();
    public bool is_empty();
    @MutSelf public String split_off(uint at);
    @MutSelf public void clear();
    @MutSelf @RustBorrowsSelf public Drain drain<R>(R range);
    public IntoChars into_chars();
    @MutSelf public void replace_range<R>(R range, &String replace_with);
    @MutSelf public void replace_first<P>(P from, &String to);
    @MutSelf public void replace_last<P>(P from, &String to);
    public String into_boxed_str();
    @RustRefOut public String leak();
    public ubyte[] into_boxed_bytes();
    public String replace<P>(P from, &String to);
    public String replacen<P>(P pat, &String to, uint count);
    public String to_lowercase();
    public String word_to_titlecase();
    public String to_uppercase();
    public String to_casefold_unnormalized();
    public String into_string();
    public String repeat(uint n);
    public String to_ascii_uppercase();
    public String to_ascii_lowercase();
    public bool is_char_boundary(uint index);
    public uint floor_char_boundary(uint index);
    public uint ceil_char_boundary(uint index);
    @MutSelf @RustRefOut public unsafe ubyte[] as_bytes_mut();
    public ubyte* as_ptr();
    @MutSelf public ubyte* as_mut_ptr();
    @RustRefOut public I.Output? get<I>(I i);
    @MutSelf @RustRefOut public I.Output? get_mut<I>(I i);
    @RustRefOut public unsafe I.Output get_unchecked<I>(I i);
    @MutSelf @RustRefOut public unsafe I.Output get_unchecked_mut<I>(I i);
    @RustRefOut public unsafe String slice_unchecked(uint begin, uint end);
    @MutSelf @RustRefOut public unsafe String slice_mut_unchecked(uint begin, uint end);
    public (String, String) split_at(uint mid);
    @MutSelf public (String, String) split_at_mut(uint mid);
    public (String, String)? split_at_checked(uint mid);
    @MutSelf public (String, String)? split_at_mut_checked(uint mid);
    @RustBorrowsSelf public Chars chars();
    @RustBorrowsSelf public CharIndices char_indices();
    @RustBorrowsSelf public Bytes bytes();
    @RustBorrowsSelf public SplitWhitespace split_whitespace();
    @RustBorrowsSelf public SplitAsciiWhitespace split_ascii_whitespace();
    @RustBorrowsSelf public Lines lines();
    @RustBorrowsSelf public LinesAny lines_any();
    @RustBorrowsSelf public EncodeUtf16 encode_utf16();
    public bool contains<P>(P pat);
    public bool starts_with<P>(P pat);
    public bool ends_with<P>(P pat);
    public uint? find<P>(P pat);
    public uint? rfind<P>(P pat);
    @RustBorrowsSelf public Split<P> split<P>(P pat);
    @RustBorrowsSelf public SplitInclusive<P> split_inclusive<P>(P pat);
    @RustBorrowsSelf public RSplit<P> rsplit<P>(P pat);
    @RustBorrowsSelf public SplitTerminator<P> split_terminator<P>(P pat);
    @RustBorrowsSelf public RSplitTerminator<P> rsplit_terminator<P>(P pat);
    @RustBorrowsSelf public SplitN<P> splitn<P>(uint n, P pat);
    @RustBorrowsSelf public RSplitN<P> rsplitn<P>(uint n, P pat);
    public (String, String)? split_once<P>(P delimiter);
    public (String, String)? rsplit_once<P>(P delimiter);
    @RustBorrowsSelf public Matches<P> matches<P>(P pat);
    @RustBorrowsSelf public RMatches<P> rmatches<P>(P pat);
    @RustBorrowsSelf public MatchIndices<P> match_indices<P>(P pat);
    @RustBorrowsSelf public RMatchIndices<P> rmatch_indices<P>(P pat);
    @RustRefOut public String trim();
    @RustRefOut public String trim_start();
    @RustRefOut public String trim_end();
    @RustRefOut public String trim_left();
    @RustRefOut public String trim_right();
    @RustRefOut public String trim_matches<P>(P pat);
    @RustRefOut public String trim_start_matches<P>(P pat);
    @RustRefOut public String? strip_prefix<P>(P prefix);
    @RustRefOut public String? strip_suffix<P>(P suffix);
    @RustRefOut public String? strip_circumfix<P, S>(P prefix, S suffix);
    @RustRefOut public String trim_prefix<P>(P prefix);
    @RustRefOut public String trim_suffix<P>(P suffix);
    @RustRefOut public String trim_end_matches<P>(P pat);
    @RustRefOut public String trim_left_matches<P>(P pat);
    @RustRefOut public String trim_right_matches<P>(P pat);
    public F parse<F>() throws Error;
    public bool is_ascii();
    @RustRefOut public AsciiChar[]? as_ascii();
    @RustRefOut public unsafe AsciiChar[] as_ascii_unchecked();
    public bool eq_ignore_ascii_case(&String other);
    public bool eq_ignore_case_unnormalized(&String other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustRefOut public String trim_ascii_start();
    @RustRefOut public String trim_ascii_end();
    @RustRefOut public String trim_ascii();
    @RustBorrowsSelf public EscapeDebug escape_debug();
    @RustBorrowsSelf public EscapeDefault escape_default();
    @RustBorrowsSelf public EscapeUnicode escape_unicode();
    public Range<uint>? substr_range(&String substr);
}

/** An error returned from [`Path::strip_prefix`] if the prefix was not found. */
@rust("std::path::StripPrefixError")
@RustClone
public class StripPrefixError {
}

/** A lazy iterator producing elements in the symmetric difference of `BTreeSet`s. */
@rust("std::collections::btree_set::SymmetricDifference")
@RustClone
public class SymmetricDifference<T> implements ToOwned {
    @MutSelf public T? next();
}

/** The sending-half of Rust's synchronous [`sync_channel`] type. */
@rust("std::sync::mpsc::SyncSender")
@RustClone
public class SyncSender<T> {
    public void send(T t) throws SendError<T>;
    public void try_send(T t) throws TrySendError<T>;
}

/** `SyncView` provides _mutable_ access, also referred to as _exclusive_ */
@rust("std::sync::SyncView")
@RustClone
public class SyncView<T> implements Any, Clone, CloneToUninit, Copy, Debug, Default, Eq, Future, Hash, IntoFuture, Ord, Pattern, StructuralPartialEq, Sync {
    public SyncView(T t);
    @RustDefault public SyncView();
    public T into_inner();
    @RustRefOut public Pin<T> as_pin_mut();
    @RustRefOut public static SyncView<T> from_mut(&mut T r);
    @RustRefOut public static Pin<SyncView<T>> from_pin_mut(Pin<T> r);
    @RustRefOut public Pin<T> as_pin_ref();
}

/** The default memory allocator provided by the operating system. */
@rust("std::alloc::System")
@RustClone
public class System {
    @RustDefault public System();
}

/** The system random number generator. */
@rust("std::random::SystemRng")
@RustClone
public class SystemRng {
    @RustDefault public SystemRng();
}

/** A measurement of the system clock, useful for talking to */
@rust("std::time::SystemTime")
@RustClone
public class SystemTime {
    public static SystemTime now();
    public Duration duration_since(SystemTime earlier) throws SystemTimeError;
    public Duration elapsed() throws SystemTimeError;
    public SystemTime? checked_add(Duration duration);
    public SystemTime? checked_sub(Duration duration);
    public SystemTime saturating_add(Duration duration);
    public SystemTime saturating_sub(Duration duration);
    public Duration saturating_duration_since(SystemTime earlier);
}

/** An error returned from the `duration_since` and `elapsed` methods on */
@rust("std::time::SystemTimeError")
@RustClone
public class SystemTimeError {
    public Duration duration();
}

/** A TCP socket server, listening for connections. */
@rust("std::net::TcpListener")
public class TcpListener implements AsFd, AsRawFd, AsRawSocket, AsSocket, FromRawFd, FromRawSocket, IntoRawFd, IntoRawSocket {
    public static TcpListener bind<A>(A addr) throws Error;
    public SocketAddr local_addr() throws Error;
    public TcpListener try_clone() throws Error;
    public (TcpStream, SocketAddr) accept() throws Error;
    @RustBorrowsSelf public Incoming incoming();
    public IntoIncoming into_incoming();
    public void set_ttl(u32 ttl) throws Error;
    public u32 ttl() throws Error;
    public void set_only_v6(bool only_v6) throws Error;
    public bool only_v6() throws Error;
    public Error? take_error() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
}

/** A TCP stream between a local and a remote socket. */
@rust("std::net::TcpStream")
public class TcpStream implements AsFd, AsRawFd, AsRawSocket, AsSocket, FromRawFd, FromRawSocket, IntoRawFd, IntoRawSocket, Read, TcpStreamExt, Write {
    public static TcpStream connect<A>(A addr) throws Error;
    public static TcpStream connect_timeout(&SocketAddr addr, Duration timeout) throws Error;
    public SocketAddr peer_addr() throws Error;
    public SocketAddr local_addr() throws Error;
    public void shutdown(Shutdown how) throws Error;
    public TcpStream try_clone() throws Error;
    public void set_read_timeout(Duration? dur) throws Error;
    public void set_write_timeout(Duration? dur) throws Error;
    public Duration? read_timeout() throws Error;
    public Duration? write_timeout() throws Error;
    public uint peek(&mut ubyte[] buf) throws Error;
    public void set_linger(Duration? linger) throws Error;
    public Duration? linger() throws Error;
    public void set_keepalive(bool keepalive) throws Error;
    public bool keepalive() throws Error;
    public void set_nodelay(bool nodelay) throws Error;
    public bool nodelay() throws Error;
    public void set_ttl(u32 ttl) throws Error;
    public u32 ttl() throws Error;
    public Error? take_error() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
}

/** Os-specific extensions for [`TcpStream`] */
@rust("std::os::linux::net::TcpStreamExt")
public interface TcpStreamExt {
    public void set_quickack(bool quickack) throws Error;
    public bool quickack() throws Error;
}

/** A trait for implementing arbitrary return types in the `main` function. */
@rust("std::process::Termination")
public interface Termination {
    public ExitCode report();
}

/** ThinBox. */
@rust("std::boxed::ThinBox")
public class ThinBox<T> implements ToString {
    public ThinBox(T value);
    public static ThinBox try_new(T value) throws AllocError;
    public static ThinBox new_unsize<T>(T value);
}

/** A handle to a thread. */
@rust("std::thread::Thread")
@RustClone
public class Thread {
    public void unpark();
    public ThreadId id();
    @RustRefOut public String? name();
    public Object* into_raw();
    public static unsafe Thread from_raw(Object* ptr);
}

/** A unique identifier for a running thread. */
@rust("std::thread::ThreadId")
@RustClone
public class ThreadId {
    public NonZero<ulong> as_u64();
}

/** A generalization of `Clone` to borrowed data. */
@rust("std::borrow::ToOwned")
public interface ToOwned {
    public Self.Owned to_owned();
    public void clone_into(&mut Self.Owned target);
}

/** A trait for objects which can be converted or resolved to one or more */
@rust("std::net::ToSocketAddrs")
public interface ToSocketAddrs {
    public Self.Iter to_socket_addrs() throws Error;
}

/** A trait for converting a value to a `String`. */
@rust("std::string::ToString")
public interface ToString {
    public String to_string();
}

/** An error which can be returned when converting a floating-point value of seconds */
@rust("std::time::TryFromFloatSecsError")
@RustClone
public class TryFromFloatSecsError implements Any, Clone, CloneToUninit, Debug, Display, Eq, Error, StructuralPartialEq {
}

/** The error type returned when a checked integral type conversion fails. */
@rust("std::num::TryFromIntError")
@RustClone
public class TryFromIntError implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Error, StructuralPartialEq {
    @RustRefOut public IntErrorKind kind();
}

/** An iterator that attempts to yield all pending values for a [`Receiver`], */
@rust("std::sync::mpmc::TryIter")
public class TryIter<T> {
    @MutSelf public T? next();
}

/** An enumeration of possible errors which can occur while trying to acquire a lock */
@rust("std::fs::TryLockError")
public enum TryLockError {
    Error(Error), WouldBlock
}

/** This enumeration is the list of the possible reasons that [`try_recv`] could */
@rust("std::sync::mpsc::TryRecvError")
@RustClone
public enum TryRecvError {
    Empty, Disconnected
}

/** The error type for `try_reserve` methods. */
@rust("std::collections::TryReserveError")
@RustClone
public class TryReserveError implements ToOwned, ToString {
    public TryReserveErrorKind kind();
}

/** Details of the allocation that caused a `TryReserveError` */
@rust("std::collections::TryReserveErrorKind")
@RustClone
public enum TryReserveErrorKind implements ToOwned {
    CapacityOverflow, AllocError
}

/** This enumeration is the list of the possible error outcomes for the */
@rust("std::sync::mpsc::TrySendError")
@RustClone
public enum TrySendError<T> {
    Full(T), Disconnected(T)
}

@rust("std::time::UNIX_EPOCH")
public const SystemTime UNIX_EPOCH;

/** A UDP socket. */
@rust("std::net::UdpSocket")
public class UdpSocket implements AsFd, AsRawFd, AsRawSocket, AsSocket, FromRawFd, FromRawSocket, IntoRawFd, IntoRawSocket {
    public static UdpSocket bind<A>(A addr) throws Error;
    public (uint, SocketAddr) recv_from(&mut ubyte[] buf) throws Error;
    public (uint, SocketAddr) peek_from(&mut ubyte[] buf) throws Error;
    public uint send_to<A>(ubyte[] buf, A addr) throws Error;
    public SocketAddr peer_addr() throws Error;
    public SocketAddr local_addr() throws Error;
    public UdpSocket try_clone() throws Error;
    public void set_read_timeout(Duration? dur) throws Error;
    public void set_write_timeout(Duration? dur) throws Error;
    public Duration? read_timeout() throws Error;
    public Duration? write_timeout() throws Error;
    public void set_broadcast(bool broadcast) throws Error;
    public bool broadcast() throws Error;
    public void set_multicast_loop_v4(bool multicast_loop_v4) throws Error;
    public bool multicast_loop_v4() throws Error;
    public void set_multicast_ttl_v4(u32 multicast_ttl_v4) throws Error;
    public u32 multicast_ttl_v4() throws Error;
    public void set_multicast_loop_v6(bool multicast_loop_v6) throws Error;
    public bool multicast_loop_v6() throws Error;
    public void set_ttl(u32 ttl) throws Error;
    public u32 ttl() throws Error;
    public void join_multicast_v4(&Ipv4Addr multiaddr, &Ipv4Addr interface) throws Error;
    public void join_multicast_v6(&Ipv6Addr multiaddr, u32 interface) throws Error;
    public void leave_multicast_v4(&Ipv4Addr multiaddr, &Ipv4Addr interface) throws Error;
    public void leave_multicast_v6(&Ipv6Addr multiaddr, u32 interface) throws Error;
    public Error? take_error() throws Error;
    public void connect<A>(A addr) throws Error;
    public uint send(ubyte[] buf) throws Error;
    public uint recv(&mut ubyte[] buf) throws Error;
    public uint peek(&mut ubyte[] buf) throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
}

/** A lazy iterator producing elements in the union of `BTreeSet`s. */
@rust("std::collections::btree_set::Union")
@RustClone
public class Union<T> implements ToOwned {
    @MutSelf public T? next();
}

/** A uniquely owned [`Arc`]. */
@rust("std::sync::UniqueArc")
public class UniqueArc<T, A> implements ToString {
    public UniqueArc(T value);
    public static UniqueArc<U> map<U>(UniqueArc this, (T) -> U f);
    public static Self.TryType try_map<R>(UniqueArc this, (T) -> R f);
    public static UniqueArc new_in(T data, A alloc);
    public static T into_arc(UniqueArc this);
    public static Weak<T, A> downgrade(&UniqueArc this);
}

/** A uniquely owned [`Rc`]. */
@rust("std::rc::UniqueRc")
public class UniqueRc<T, A> implements ToString {
    public UniqueRc(T value);
    public static UniqueRc<U> map<U>(UniqueRc this, (T) -> U f);
    public static Self.TryType try_map<R>(UniqueRc this, (T) -> R f);
    public static UniqueRc new_in(T value, A alloc);
    public static T into_rc(UniqueRc this);
    public static Weak<T, A> downgrade(&UniqueRc this);
}

/** A Unix datagram socket. */
@rust("std::os::unix::net::UnixDatagram")
public class UnixDatagram implements AsFd, AsRawFd, FromRawFd, IntoRawFd, UnixSocketExt {
    public static UnixDatagram bind<P>(P path) throws Error;
    public static UnixDatagram bind_addr(&SocketAddr socket_addr) throws Error;
    public static UnixDatagram unbound() throws Error;
    public static (UnixDatagram, UnixDatagram) pair() throws Error;
    public void connect<P>(P path) throws Error;
    public void connect_addr(&SocketAddr socket_addr) throws Error;
    public UnixDatagram try_clone() throws Error;
    public SocketAddr local_addr() throws Error;
    public SocketAddr peer_addr() throws Error;
    public (uint, SocketAddr) recv_from(&mut ubyte[] buf) throws Error;
    public uint recv(&mut ubyte[] buf) throws Error;
    public (uint, bool, SocketAddr) recv_vectored_with_ancillary_from(&mut IoSliceMut[] bufs, &mut SocketAncillary ancillary) throws Error;
    public (uint, bool) recv_vectored_with_ancillary(&mut IoSliceMut[] bufs, &mut SocketAncillary ancillary) throws Error;
    public uint send_to<P>(ubyte[] buf, P path) throws Error;
    public uint send_to_addr(ubyte[] buf, &SocketAddr socket_addr) throws Error;
    public uint send(ubyte[] buf) throws Error;
    public uint send_vectored_with_ancillary_to<P>(IoSlice[] bufs, &mut SocketAncillary ancillary, P path) throws Error;
    public uint send_vectored_with_ancillary(IoSlice[] bufs, &mut SocketAncillary ancillary) throws Error;
    public void set_read_timeout(Duration? timeout) throws Error;
    public void set_write_timeout(Duration? timeout) throws Error;
    public Duration? read_timeout() throws Error;
    public Duration? write_timeout() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
    public void set_mark(u32 mark) throws Error;
    public Error? take_error() throws Error;
    public void shutdown(Shutdown how) throws Error;
    public uint peek(&mut ubyte[] buf) throws Error;
    public (uint, SocketAddr) peek_from(&mut ubyte[] buf) throws Error;
}

/** A structure representing a Unix domain socket server. */
@rust("std::os::unix::net::UnixListener")
public class UnixListener implements AsFd, AsRawFd, FromRawFd, IntoRawFd {
    public static UnixListener bind<P>(P path) throws Error;
    public static UnixListener bind_addr(&SocketAddr socket_addr) throws Error;
    public (UnixStream, SocketAddr) accept() throws Error;
    public UnixListener try_clone() throws Error;
    public SocketAddr local_addr() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
    public Error? take_error() throws Error;
    @RustBorrowsSelf public Incoming incoming();
}

/** Linux-specific functionality for `AF_UNIX` sockets [`UnixDatagram`] */
@rust("std::os::linux::net::UnixSocketExt")
public interface UnixSocketExt {
    public bool passcred() throws Error;
    public void set_passcred(bool passcred) throws Error;
}

/** A Unix stream socket. */
@rust("std::os::unix::net::UnixStream")
public class UnixStream implements AsFd, AsRawFd, FromRawFd, IntoRawFd, Read, UnixSocketExt, Write {
    public static UnixStream connect<P>(P path) throws Error;
    public static UnixStream connect_addr(&SocketAddr socket_addr) throws Error;
    public static (UnixStream, UnixStream) pair() throws Error;
    public UnixStream try_clone() throws Error;
    public SocketAddr local_addr() throws Error;
    public SocketAddr peer_addr() throws Error;
    public void set_read_timeout(Duration? timeout) throws Error;
    public void set_write_timeout(Duration? timeout) throws Error;
    public Duration? read_timeout() throws Error;
    public Duration? write_timeout() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
    public void set_mark(u32 mark) throws Error;
    public Error? take_error() throws Error;
    public void shutdown(Shutdown how) throws Error;
    public uint peek(&mut ubyte[] buf) throws Error;
    public uint recv_vectored_with_ancillary(&mut IoSliceMut[] bufs, &mut SocketAncillary ancillary) throws Error;
    public uint send_vectored_with_ancillary(IoSlice[] bufs, &mut SocketAncillary ancillary) throws Error;
}

/** Error type returned by [`CursorMut::insert_before`] and */
@rust("std::collections::btree_map::UnorderedKeyError")
@RustClone
public class UnorderedKeyError implements ToOwned, ToString {
}

/** An iterator used to decode a slice of mostly UTF-8 bytes to string slices */
@rust("std::str::Utf8Chunks")
@RustClone
public class Utf8Chunks implements Any, Clone, CloneToUninit, Debug, FusedIterator, IntoIterator, Iterator {
    @MutSelf public Utf8Chunk? next();
}

/** Errors which can occur when attempting to interpret a sequence of [`u8`] */
@rust("std::str::Utf8Error")
@RustClone
public class Utf8Error implements Any, Clone, CloneToUninit, Copy, Debug, Display, Eq, Error, StructuralPartialEq {
    public uint valid_up_to();
    public uint? error_len();
}

/** A variable argument list, ABI-compatible with `va_list` in C. */
@rust("std::ffi::VaList")
@RustClone
public class VaList implements Any, Clone, CloneToUninit, Debug, Drop {
    @MutSelf public unsafe T next_arg<T>();
}

/** A view into a vacant entry in a `BTreeMap`. */
@rust("std::collections::btree_map::VacantEntry")
public class VacantEntry<K, V, A> {
    @RustRefOut public K key();
    public K into_key();
    @RustRefOut public V insert(V value);
    @RustBorrowsSelf public OccupiedEntry<K, V, A> insert_entry(V value);
}

/** An iterator over the values of a `BTreeMap`. */
@rust("std::collections::btree_map::Values")
@RustClone
public class Values<K, V> implements ToOwned {
    @RustDefault public Values();
    @MutSelf public V? next();
}

/** A mutable iterator over the values of a `BTreeMap`. */
@rust("std::collections::btree_map::ValuesMut")
public class ValuesMut<K, V> {
    @RustDefault public ValuesMut();
    @MutSelf public V? next();
}

/** The error type for operations interacting with environment variables. */
@rust("std::env::VarError")
@RustClone
public enum VarError {
    NotPresent, NotUnicode(OsString)
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::Vars")
public class Vars {
    @MutSelf public (String, String)? next();
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::VarsOs")
public class VarsOs {
    @MutSelf public (OsString, OsString)? next();
}

/** A contiguous growable array type, written as `Vec<T>`, short for 'vector'. */
@rust("std::vec::Vec")
@RustClone
@RustCollection
public class Vec<T, A> implements ToOwned {
    public Vec();
    public static Vec with_capacity(uint capacity);
    public static Vec try_with_capacity(uint capacity) throws TryReserveError;
    public static unsafe Vec from_raw_parts(T* ptr, uint length, uint capacity);
    public static unsafe Vec from_parts(NonNull<T> ptr, uint length, uint capacity);
    public static Vec from_fn<F>(uint length, (uint) -> T f);
    public (T*, uint, uint) into_raw_parts();
    public (NonNull<T>, uint, uint) into_parts();
    @RustRefOut public T[] const_make_global();
    public static Vec with_capacity_in(uint capacity, A alloc);
    @MutSelf public void push(T value);
    @MutSelf @RustRefOut public T push_mut(T value);
    public static Vec new_in(A alloc);
    public static Vec try_with_capacity_in(uint capacity, A alloc) throws TryReserveError;
    public static unsafe Vec from_raw_parts_in(T* ptr, uint length, uint capacity, A alloc);
    public static unsafe Vec from_parts_in(NonNull<T> ptr, uint length, uint capacity, A alloc);
    public (T*, uint, uint, A) into_raw_parts_with_alloc();
    public (NonNull<T>, uint, uint, A) into_parts_with_alloc();
    public uint capacity();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf public void try_shrink_to_fit() throws TryReserveError;
    @MutSelf public void try_shrink_to(uint min_capacity) throws TryReserveError;
    public T[] into_boxed_slice();
    public T[] into_array() throws Vec;
    @MutSelf public void truncate(uint len);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public T[] as_mut_slice();
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    @MutSelf public NonNull<T> as_non_null();
    @RustRefOut public A allocator();
    @MutSelf public unsafe void set_len(uint new_len);
    @MutSelf public T swap_remove(uint index);
    @MutSelf public void insert(uint index, T element);
    @MutSelf @RustRefOut public T insert_mut(uint index, T element);
    @MutSelf public T remove(uint index);
    @MutSelf public T? try_remove(uint index);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void retain_mut<F>((T) -> bool f);
    @MutSelf public void dedup_by_key<F, K>((T) -> K key);
    @MutSelf public void dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf @RustRefOut public T push_within_capacity(T value) throws T;
    @MutSelf public T? pop();
    @MutSelf public T? pop_if((T) -> bool predicate);
    @MutSelf @RustBorrowsSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public void append(&mut Vec other);
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public void clear();
    public uint len();
    public bool is_empty();
    @MutSelf public Vec split_off(uint at);
    @MutSelf public void resize_with<F>(uint new_len, () -> T f);
    @RustRefOut public T[] leak();
    @MutSelf @RustRefOut public MaybeUninit<T>[] spare_capacity_mut();
    @MutSelf public (T[], MaybeUninit<T>[]) split_at_spare_mut();
    public Vec<T[]> into_chunks();
    public Vec<U> recycle<U>();
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_slice(T[] other);
    @MutSelf public void extend_from_within<R>(R src);
    public Vec<T> into_flattened();
    @MutSelf public void dedup();
    @MutSelf @RustBorrowsSelf public Splice<I.IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf @RustBorrowsSelf public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
    @MutSelf public void sort_floats();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
    public bool is_ascii();
    @RustRefOut public Char[]? as_ascii();
    @RustRefOut public unsafe Char[] as_ascii_unchecked();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    @RustRefOut public T? first();
    @MutSelf @RustRefOut public T? first_mut();
    public (T, T[])? split_first();
    @MutSelf public (T, T[])? split_first_mut();
    public (T, T[])? split_last();
    @MutSelf public (T, T[])? split_last_mut();
    @RustRefOut public T? last();
    @MutSelf @RustRefOut public T? last_mut();
    @RustRefOut public T[]? first_chunk();
    @MutSelf @RustRefOut public T[]? first_chunk_mut();
    public (T[], T[])? split_first_chunk();
    @MutSelf public (T[], T[])? split_first_chunk_mut();
    public (T[], T[])? split_last_chunk();
    @MutSelf public (T[], T[])? split_last_chunk_mut();
    @RustRefOut public T[]? last_chunk();
    @MutSelf @RustRefOut public T[]? last_chunk_mut();
    @RustRefOut public I.Output? get<I>(I index);
    @MutSelf @RustRefOut public I.Output? get_mut<I>(I index);
    @RustRefOut public unsafe I.Output get_unchecked<I>(I index);
    @MutSelf @RustRefOut public unsafe I.Output get_unchecked_mut<I>(I index);
    public Range<T*> as_ptr_range();
    @MutSelf public Range<T*> as_mut_ptr_range();
    @RustRefOut public T[]? as_array();
    @MutSelf @RustRefOut public T[]? as_mut_array();
    @MutSelf public void swap(uint a, uint b);
    @MutSelf public unsafe void swap_unchecked(uint a, uint b);
    @MutSelf public void reverse();
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    @RustBorrowsSelf public Windows<T> windows(uint size);
    @RustBorrowsSelf public Chunks<T> chunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksMut<T> chunks_mut(uint chunk_size);
    @RustBorrowsSelf public ChunksExact<T> chunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public ChunksExactMut<T> chunks_exact_mut(uint chunk_size);
    @RustRefOut public unsafe T[][] as_chunks_unchecked();
    public (T[][], T[]) as_chunks();
    public (T[], T[][]) as_rchunks();
    @MutSelf @RustRefOut public unsafe T[][] as_chunks_unchecked_mut();
    @MutSelf public (T[][], T[]) as_chunks_mut();
    @MutSelf public (T[], T[][]) as_rchunks_mut();
    @RustBorrowsSelf public ArrayWindows<T> array_windows();
    @RustBorrowsSelf public RChunks<T> rchunks(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksMut<T> rchunks_mut(uint chunk_size);
    @RustBorrowsSelf public RChunksExact<T> rchunks_exact(uint chunk_size);
    @MutSelf @RustBorrowsSelf public RChunksExactMut<T> rchunks_exact_mut(uint chunk_size);
    @RustBorrowsSelf public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    public (T[], T[])? split_once<F>((T) -> bool pred);
    public (T[], T[])? rsplit_once<F>((T) -> bool pred);
    public bool contains(&T x);
    public bool starts_with(T[] needle);
    public bool ends_with(T[] needle);
    @RustRefOut public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut public T[]? strip_suffix<P>(&P suffix);
    @RustRefOut public T[]? strip_circumfix<S, P>(&P prefix, &S suffix);
    @RustRefOut public T[] trim_prefix<P>(&P prefix);
    @RustRefOut public T[] trim_suffix<P>(&P suffix);
    public uint binary_search(&T x) throws Error;
    public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf public void sort_unstable();
    @MutSelf public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf public void partial_sort_unstable<R>(R range);
    @MutSelf public void partial_sort_unstable_by<F, R>(R range, (T, T) -> Ordering compare);
    @MutSelf public void partial_sort_unstable_by_key<K, F, R>(R range, (T) -> K f);
    @MutSelf public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public (T[], T[]) partition_dedup();
    @MutSelf public (T[], T[]) partition_dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public (T[], T[]) partition_dedup_by_key<K, F>((T) -> K key);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf public T[] shift_left(T[] inserted);
    @MutSelf public T[] shift_right(T[] inserted);
    @MutSelf public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf public void clone_from_slice(T[] src);
    @MutSelf public void copy_from_slice(T[] src);
    @MutSelf public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    public (T[], Simd<T>[], T[]) as_simd();
    @MutSelf public (T[], Simd<T>[], T[]) as_simd_mut();
    public bool is_sorted();
    public bool is_sorted_by<F>((T, T) -> bool compare);
    public bool is_sorted_by_key<F, K>((T) -> K f);
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public Range<uint>? subslice_range(T[] subslice);
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
}

/** A double-ended queue implemented with a growable ring buffer. */
@rust("std::collections::VecDeque")
@RustClone
@RustCollection
public class VecDeque<T, A> implements ToOwned {
    public VecDeque();
    @MutSelf @RustBorrowsSelf public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
    public static VecDeque<T> with_capacity(uint capacity);
    public static VecDeque<T> try_with_capacity(uint capacity) throws TryReserveError;
    public static VecDeque<T, A> new_in(A alloc);
    public static VecDeque<T, A> with_capacity_in(uint capacity, A alloc);
    @RustRefOut public T? get(uint index);
    @MutSelf @RustRefOut public T? get_mut(uint index);
    @MutSelf public void swap(uint i, uint j);
    public uint capacity();
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf public void truncate(uint len);
    @MutSelf public void truncate_front(uint len);
    @RustRefOut public A allocator();
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    public (T[], T[]) as_slices();
    @MutSelf public (T[], T[]) as_mut_slices();
    public uint len();
    public bool is_empty();
    @RustBorrowsSelf public Iter<T> range<R>(R range);
    @MutSelf @RustBorrowsSelf public IterMut<T> range_mut<R>(R range);
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain<R>(R range);
    @MutSelf @RustBorrowsSelf public Splice<I.IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf public void clear();
    public bool contains(&T x);
    @RustRefOut public T? front();
    @MutSelf @RustRefOut public T? front_mut();
    @RustRefOut public T? back();
    @MutSelf @RustRefOut public T? back_mut();
    @MutSelf public T? pop_front();
    @MutSelf public T? pop_back();
    @MutSelf public T? pop_front_if((T) -> bool predicate);
    @MutSelf public T? pop_back_if((T) -> bool predicate);
    @MutSelf public void push_front(T value);
    @MutSelf @RustRefOut public T push_front_mut(T value);
    @MutSelf public void push_back(T value);
    @MutSelf @RustRefOut public T push_back_mut(T value);
    @MutSelf public void prepend<I>(I other);
    @MutSelf public void extend_front<I>(I iter);
    @MutSelf public T? swap_remove_front(uint index);
    @MutSelf public T? swap_remove_back(uint index);
    @MutSelf public void insert(uint index, T value);
    @MutSelf @RustRefOut public T insert_mut(uint index, T value);
    @MutSelf public T? remove(uint index);
    @MutSelf public VecDeque split_off(uint at);
    @MutSelf public void append(&mut VecDeque other);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void retain_mut<F>((T) -> bool f);
    @MutSelf public void resize_with(uint new_len, () -> T generator);
    @MutSelf @RustRefOut public T[] make_contiguous();
    @MutSelf public void rotate_left(uint n);
    @MutSelf public void rotate_right(uint n);
    public uint binary_search(&T x) throws Error;
    public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_within<R>(R src);
    @MutSelf public void prepend_from_within<R>(R src);
}

/** A type indicating whether a timed wait on a condition variable returned */
@rust("std::sync::WaitTimeoutResult")
@RustClone
public class WaitTimeoutResult {
    public bool timed_out();
}

/** The implementation of waking a task on an executor. */
@rust("std::task::Wake")
public interface Wake {
    public void wake();
    public void wake_by_ref();
}

/** A `Waker` is a handle for waking up a task by notifying its executor that it */
@rust("std::task::Waker")
@RustClone
public class Waker implements Any, Clone, CloneToUninit, Debug, Drop, Send, Sync, Unpin {
    public Waker(Object* data, &RawWakerVTable vtable);
    public void wake();
    public void wake_by_ref();
    public bool will_wake(&Waker other);
    public static unsafe Waker from_raw(RawWaker waker);
    @RustRefOut public static Waker noop();
    public Object* data();
    @RustRefOut public RawWakerVTable vtable();
    public static Waker from_fn_ptr(() -> void f);
}

/** `Weak` is a version of [`Rc`] that holds a non-owning reference to the */
@rust("std::rc::Weak")
@RustClone
public class Weak<T, A> implements ToOwned {
    public Weak();
    public static Weak<T, A> new_in(A alloc);
    public static unsafe Weak from_raw(T* ptr);
    public T* into_raw();
    @RustRefOut public A allocator();
    public T* as_ptr();
    public (T*, A) into_raw_with_allocator();
    public static unsafe Weak from_raw_in(T* ptr, A alloc);
    public T? upgrade();
    public uint strong_count();
    public uint weak_count();
    public bool ptr_eq(&Weak other);
}

/** An iterator over overlapping subslices of length `size`. */
@rust("std::slice::Windows")
@RustClone
public class Windows<T> implements Any, Clone, CloneToUninit, Debug, DoubleEndedIterator, ExactSizeIterator, FusedIterator, IntoIterator, Iterator, TrustedLen {
    @MutSelf public T[]? next();
}

/** A lock could not be acquired at this time because the operation would otherwise block. */
@rust("std::sync::nonpoison::WouldBlock")
public class WouldBlock {
}

/** Provides intentionally-wrapped arithmetic on `T`. */
@rust("std::num::Wrapping")
@RustClone
public class Wrapping<T> implements Any, Binary, Clone, CloneToUninit, Copy, Debug, Default, Display, Eq, Hash, LowerHex, Octal, Ord, StructuralPartialEq, UpperHex {
    @RustDefault public Wrapping();
    public u32 count_ones();
    public u32 count_zeros();
    public u32 trailing_zeros();
    public Wrapping rotate_left(u32 n);
    public Wrapping rotate_right(u32 n);
    public Wrapping swap_bytes();
    public Wrapping reverse_bits();
    public static Wrapping from_be(Wrapping x);
    public static Wrapping from_le(Wrapping x);
    public Wrapping to_be();
    public Wrapping to_le();
    public Wrapping pow(u32 exp);
    public u32 leading_zeros();
    public Wrapping<int> abs();
    public Wrapping<int> signum();
    public bool is_positive();
    public bool is_negative();
    public bool is_power_of_two();
    public Wrapping next_power_of_two();
}

/** A trait for objects which are byte-oriented sinks. */
@rust("std::io::Write")
public interface Write {
    @MutSelf public uint write(ubyte[] buf) throws Error;
    @MutSelf public uint write_vectored(IoSlice[] bufs) throws Error;
    public bool is_write_vectored();
    @MutSelf public void flush() throws Error;
    @MutSelf public void write_all(ubyte[] buf) throws Error;
    @MutSelf public void write_all_vectored(&mut IoSlice[] bufs) throws Error;
    @MutSelf public void write_fmt(Arguments args) throws Error;
    @MutSelf @RustRefOut public Self by_ref();
}

/** Error returned for the buffered data from `BufWriter::into_parts`, when the underlying */
@rust("std::io::WriterPanicked")
public class WriterPanicked {
    public Vec<ubyte> into_inner();
}

/** An iterator that produces directory paths from XDG environment configuration. */
@rust("std::os::unix::xdg::XdgDirsIter")
@RustClone
public class XdgDirsIter {
    @MutSelf public PathBuf? next();
}

@rust("std::process::abort")
public never abort();

@rust("std::process::abort_immediate")
public never abort_immediate();

@rust("std::panic::abort_unwind")
public R abort_unwind<F, R>(() -> R f);

@rust("std::path::absolute")
public PathBuf absolute<P>(P path) throws Error;

@rust("std::thread::add_spawn_hook")
public void add_spawn_hook<F, G>((Thread) -> G hook);

@rust("std::mem::align_of")
public uint align_of<T>();

@rust("std::mem::align_of_val")
public uint align_of_val<T>(&T val);

@rust("std::alloc::alloc")
public unsafe ubyte* alloc(Layout layout);

@rust("std::alloc::alloc_zeroed")
public unsafe ubyte* alloc_zeroed(Layout layout);

@rust("std::panic::always_abort")
public void always_abort();

@rust("std::env::args")
public Args args();

@rust("std::env::args_os")
public ArgsOs args_os();

@rust("std::thread::available_parallelism")
public NonZero<uint> available_parallelism() throws Error;

@rust("std::panicking::begin_panic")
public never begin_panic<M>(M msg);

public type blkcnt_t = ulong;


public type blksize_t = ulong;


/** Equivalent to C's `void` type when used as a [pointer]. */
@rust("std::ffi::c_void")
public enum c_void implements Any, Debug {
    // (variants not represented)
}

@rust("std::os::unix::xdg::cache_home_dir")
public PathBuf cache_home_dir();

@rust("std::fs::canonicalize")
public PathBuf canonicalize<P>(P path) throws Error;

@rust("std::panic::catch_unwind")
public R catch_unwind<F, R>(() -> R f) throws Error;

@rust("std::sync::mpmc::channel")
public (Sender<T>, Receiver<T>) channel<T>();

@rust("std::os::unix::fs::chown")
public void chown<P>(P dir, u32? uid, u32? gid) throws Error;

@rust("std::os::unix::fs::chroot")
public void chroot<P>(P dir) throws Error;

@rust("std::os::unix::xdg::config_dirs")
public XdgDirsIter config_dirs();

@rust("std::os::unix::xdg::config_home_dir")
public PathBuf config_home_dir();

@rust("std::fs::copy")
public ulong copy<P, Q>(P from, Q to) throws Error;

@rust("std::fs::create_dir")
public void create_dir<P>(P path) throws Error;

@rust("std::fs::create_dir_all")
public void create_dir_all<P>(P path) throws Error;

@rust("std::thread::current")
public Thread current();

@rust("std::env::current_dir")
public PathBuf current_dir() throws Error;

@rust("std::env::current_exe")
public PathBuf current_exe() throws Error;

@rust("std::thread::current_id")
public ThreadId current_id();

@rust("std::os::unix::xdg::data_dirs")
public XdgDirsIter data_dirs();

@rust("std::os::unix::xdg::data_home_dir")
public PathBuf data_home_dir();

@rust("std::alloc::dealloc")
public unsafe void dealloc(ubyte* ptr, Layout layout);

public type dev_t = ulong;


@rust("std::mem::drop")
public void drop<T>(T _x);

@rust("std::io::empty")
public Empty empty();

@rust("std::ascii::escape_default")
public EscapeDefault escape_default(ubyte c);

@rust("std::fs::exists")
public bool exists<P>(P path) throws Error;

@rust("std::process::exit")
public never exit(i32 code);

@rust("std::os::unix::fs::fchown")
public void fchown<F>(F fd, u32? uid, u32? gid) throws Error;

@rust("std::fmt::format")
public String format(Arguments args);

@rust("std::str::from_boxed_utf8_unchecked")
public unsafe String from_boxed_utf8_unchecked(ubyte[] v);

@rust("std::panic::get_backtrace_style")
public BacktraceStyle? get_backtrace_style();

public type gid_t = u32;


@rust("std::alloc::handle_alloc_error")
public never handle_alloc_error(Layout layout);

@rust("std::fs::hard_link")
public void hard_link<P, Q>(P original, Q link) throws Error;

@rust("std::env::home_dir")
public PathBuf? home_dir();

@rust("std::net::hostname")
public OsString hostname() throws Error;

@rust("std::process::id")
public u32 id();

public type ino_t = ulong;


@rust("std::path::is_separator")
public bool is_separator(char c);

@rust("std::env::join_paths")
public OsString join_paths<I, T>(I paths) throws JoinPathsError;

@rust("std::os::windows::fs::junction_point")
public void junction_point<P, Q>(P original, Q link) throws Error;

@rust("std::os::unix::fs::lchown")
public void lchown<P>(P dir, u32? uid, u32? gid) throws Error;

@rust("std::task::local_waker_fn")
public LocalWaker local_waker_fn<F>(() -> void f);

@rust("std::fs::metadata")
public Metadata metadata<P>(P path) throws Error;

@rust("std::os::unix::fs::mkfifo")
public void mkfifo<P>(P path, Permissions permissions) throws Error;

public type mode_t = u32;


public type nlink_t = ulong;


public type off_t = ulong;


@rust("std::panic::panic_any")
public never panic_any<M>(M msg);

@rust("std::thread::panicking")
public bool panicking();

@rust("std::os::unix::process::parent_id")
public u32 parent_id();

@rust("std::thread::park")
public void park();

@rust("std::thread::park_timeout")
public void park_timeout(Duration dur);

@rust("std::thread::park_timeout_ms")
public void park_timeout_ms(u32 ms);

public type pid_t = i32;


@rust("std::io::pipe")
public (PipeReader, PipeWriter) pipe() throws Error;

@rust("std::random::random")
public T random<T>(Distribution dist);

@rust("std::fs::read")
public Vec<ubyte> read<P>(P path) throws Error;

@rust("std::fs::read_dir")
public ReadDir read_dir<P>(P path) throws Error;

@rust("std::fs::read_link")
public PathBuf read_link<P>(P path) throws Error;

@rust("std::fs::read_to_string")
public String read_to_string<P>(P path) throws Error;

@rust("std::alloc::realloc")
public unsafe ubyte* realloc(ubyte* ptr, Layout layout, uint new_size);

@rust("std::fs::remove_dir")
public void remove_dir<P>(P path) throws Error;

@rust("std::fs::remove_dir_all")
public void remove_dir_all<P>(P path) throws Error;

@rust("std::fs::remove_file")
public void remove_file<P>(P path) throws Error;

@rust("std::env::remove_var")
public unsafe void remove_var<K>(K key);

@rust("std::fs::rename")
public void rename<P, Q>(P from, Q to) throws Error;

@rust("std::io::repeat")
public Repeat repeat(ubyte byte);

@rust("std::error::request_ref")
@RustRefOut public T? request_ref<T>(&Error err);

@rust("std::error::request_value")
public T? request_value<T>(&Error err);

@rust("std::panic::resume_unwind")
public never resume_unwind(Any payload);

@rust("std::thread::scope")
public T scope<F, T>((Scope) -> T f);

@rust("std::alloc::set_alloc_error_hook")
public void set_alloc_error_hook((Layout) -> void hook);

@rust("std::panic::set_backtrace_style")
public void set_backtrace_style(BacktraceStyle style);

@rust("std::env::set_current_dir")
public void set_current_dir<P>(P path) throws Error;

@rust("std::panic::set_hook")
public void set_hook(Fn hook);

@rust("std::fs::set_permissions")
public void set_permissions<P>(P path, Permissions perm) throws Error;

@rust("std::fs::set_permissions_nofollow")
public void set_permissions_nofollow<P>(P path, Permissions perm) throws Error;

@rust("std::fs::set_times")
public void set_times<P>(P path, FileTimes times) throws Error;

@rust("std::fs::set_times_nofollow")
public void set_times_nofollow<P>(P path, FileTimes times) throws Error;

@rust("std::env::set_var")
public unsafe void set_var<K, V>(K key, V value);

@rust("std::io::sink")
public Sink sink();

@rust("std::mem::size_of")
public uint size_of<T>();

@rust("std::mem::size_of_val")
public uint size_of_val<T>(&T val);

@rust("std::thread::sleep")
public void sleep(Duration dur);

@rust("std::thread::sleep_ms")
public void sleep_ms(u32 ms);

@rust("std::thread::sleep_until")
public void sleep_until(Instant deadline);

@rust("std::fs::soft_link")
public void soft_link<P, Q>(P original, Q link) throws Error;

@rust("std::thread::spawn")
public JoinHandle<T> spawn<F, T>(() -> T f);

@rust("std::env::split_paths")
public SplitPaths split_paths<T>(&T unparsed);

@rust("std::os::darwin::raw::stat")
public struct stat {
    public i32 st_dev;
    public ushort st_mode;
    public ushort st_nlink;
    public ulong st_ino;
    public u32 st_uid;
    public u32 st_gid;
    public i32 st_rdev;
    public c_long st_atime;
    public c_long st_atime_nsec;
    public c_long st_mtime;
    public c_long st_mtime_nsec;
    public c_long st_ctime;
    public c_long st_ctime_nsec;
    public c_long st_birthtime;
    public c_long st_birthtime_nsec;
    public long st_size;
    public long st_blocks;
    public i32 st_blksize;
    public u32 st_flags;
    public u32 st_gen;
    public i32 st_lspare;
    public long[2] st_qspare;
}

@rust("std::os::unix::xdg::state_home_dir")
public PathBuf state_home_dir();

@rust("std::io::stderr")
public Stderr stderr();

@rust("std::io::stdin")
public Stdin stdin();

@rust("std::io::stdout")
public Stdout stdout();

@rust("std::os::unix::fs::symlink")
public void symlink<P, Q>(P original, Q link) throws Error;

@rust("std::os::windows::fs::symlink_dir")
public void symlink_dir<P, Q>(P original, Q link) throws Error;

@rust("std::os::windows::fs::symlink_file")
public void symlink_file<P, Q>(P original, Q link) throws Error;

@rust("std::fs::symlink_metadata")
public Metadata symlink_metadata<P>(P path) throws Error;

@rust("std::os::wasi::fs::symlink_path")
public void symlink_path<P, U>(P old_path, U new_path) throws Error;

@rust("std::sync::mpmc::sync_channel")
public (Sender<T>, Receiver<T>) sync_channel<T>(uint cap);

@rust("std::alloc::take_alloc_error_hook")
public (Layout) -> void take_alloc_error_hook();

@rust("std::panic::take_hook")
public Fn take_hook();

@rust("std::env::temp_dir")
public PathBuf temp_dir();

public type time_t = long;


public type uid_t = u32;


@rust("std::panic::update_hook")
public void update_hook<F>((Fn, PanicHookInfo) -> void hook_fn);

@rust("std::env::var")
public String var<K>(K key) throws VarError;

@rust("std::env::var_os")
public OsString? var_os<K>(K key);

@rust("std::env::vars")
public Vars vars();

@rust("std::env::vars_os")
public VarsOs vars_os();

@rust("std::task::waker_fn")
public Waker waker_fn<F>(() -> void f);

@rust("std::fs::write")
public void write<P, C>(P path, C contents) throws Error;

@rust("std::intrinsics::write_box_via_move")
public MaybeUninit<T> write_box_via_move<T>(MaybeUninit<T> b, T x);

@rust("std::thread::yield_now")
public void yield_now();
