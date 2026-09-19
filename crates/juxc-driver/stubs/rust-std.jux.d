// juxc rust.std stub cache-version 42
// bindgen -- generated from 2 rustdoc JSON crate(s) (format_version 58)

package rust.std;

@rust("std::env::consts::ARCH")
public const String ARCH;

/** An error returned by [`LocalKey::try_with`](struct.LocalKey.html#method.try_with). */
@rust("std::thread::AccessError")
@RustClone
public class AccessError implements ToOwned, ToString {
}

/** An error which can be returned when parsing an IP address or a socket address. */
@rust("std::net::AddrParseError")
@RustClone
public class AddrParseError {
}

/** An iterator over [`Path`] and its ancestors. */
@rust("std::path::Ancestors")
@RustClone
public class Ancestors implements RustIterator, ToOwned {
    @MutSelf @RustRefOut public Path? next();
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
    @RustClosureRefs("0") public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T data);
    public static T try_unwrap(Arc this) throws Arc;
    public static T? into_inner(Arc this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public T[] into_array() throws Arc;
    public unsafe T assume_init();
    public static unsafe Arc from_raw(T* ptr);
    public static T* into_raw(Arc this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    public static T* as_ptr(&Arc this);
    @RustBounds("A: Clone") public static Weak<T, A> downgrade(&Arc this);
    public static uint weak_count(&Arc this);
    public static uint strong_count(&Arc this);
    public static bool ptr_eq(&Arc this, &Arc other);
    @RustRefOut public static T make_mut(&mut Arc this);
    public static T unwrap_or_clone(Arc this);
    @RustRefOut public static T? get_mut(&mut Arc this);
    public T downcast<T>() throws Arc;
    public unsafe T downcast_unchecked<T>();
}

/** An iterator over the arguments of a process, yielding a [`String`] value for */
@rust("std::env::Args")
public class Args implements RustIterator {
    @MutSelf public String? next();
}

/** An iterator over the arguments of a process, yielding an [`OsString`] value */
@rust("std::env::ArgsOs")
public class ArgsOs implements RustIterator {
    @MutSelf public OsString? next();
}

/** This structure represents a safely precompiled version of a format string */
@rust("std::fmt::Arguments")
@RustClone
public class Arguments {
    @RustRefOut public String? as_str();
}

/** A windowed iterator over a slice in overlapping chunks (`N` elements at a */
@rust("std::slice::ArrayWindows")
@RustClone
public class ArrayWindows<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
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

/** Extension methods for ASCII-subset only operations. */
@rust("std::ascii::AsciiExt")
@RustImplementedBy("[]")
@RustImplementedBy("char")
@RustImplementedBy("str")
@RustImplementedBy("u8")
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
public class AssertUnwindSafe<T> {
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
    @RustRefOut @RustBounds("K: Borrow, K: Ord") public V? get<Q>(&Q key);
    @RustBounds("K: Borrow, K: Ord") public (K, V)? get_key_value<Q>(&Q k);
    @RustBounds("K: Ord") public (K, V)? first_key_value();
    @MutSelf @RustBorrowsSelf @RustBounds("K: Ord") public OccupiedEntry<K, V, A>? first_entry();
    @MutSelf @RustBounds("K: Ord") public (K, V)? pop_first();
    @RustBounds("K: Ord") public (K, V)? last_key_value();
    @MutSelf @RustBorrowsSelf @RustBounds("K: Ord") public OccupiedEntry<K, V, A>? last_entry();
    @MutSelf @RustBounds("K: Ord") public (K, V)? pop_last();
    @RustBounds("K: Borrow, K: Ord") public bool contains_key<Q>(&Q key);
    @MutSelf @RustRefOut @RustBounds("K: Borrow, K: Ord") public V? get_mut<Q>(&Q key);
    @MutSelf @RustBounds("K: Ord") public V? insert(K key, V value);
    @MutSelf @RustBorrowsSelf @RustRefOut @RustBounds("K: Ord") public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf @RustBounds("K: Borrow, K: Ord") public V? remove<Q>(&Q key);
    @MutSelf @RustBounds("K: Borrow, K: Ord") public (K, V)? remove_entry<Q>(&Q key);
    @MutSelf @RustClosureRefs("0") @RustBounds("K: Ord") public void retain<F>((K, V) -> bool f);
    @MutSelf @RustBounds("K: Ord, A: Clone") public void append(&mut BTreeMap other);
    @MutSelf @RustClosureRefs("1") @RustBounds("K: Ord, A: Clone") public void merge(BTreeMap other, (K, V, V) -> V conflict);
    @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public Range<K, V> range<T, R>(R range);
    @MutSelf @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public RangeMut<K, V> range_mut<T, R>(R range);
    @MutSelf @RustBorrowsSelf @RustBounds("K: Ord") public Entry<K, V, A> entry(K key);
    @MutSelf @RustBounds("K: Borrow, K: Ord, A: Clone") public BTreeMap split_off<Q>(&Q key);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") @RustBounds("K: Ord") public ExtractIf<K, V, R, F, A> extract_if<F, R>(R range, (K, V) -> bool pred);
    public IntoKeys<K, V, A> into_keys();
    public IntoValues<K, V, A> into_values();
    @RustBorrowsSelf public Iter<K, V> iter();
    @MutSelf @RustBorrowsSelf public IterMut<K, V> iter_mut();
    @RustBorrowsSelf public Keys<K, V> keys();
    @RustBorrowsSelf public Values<K, V> values();
    @MutSelf @RustBorrowsSelf public ValuesMut<K, V> values_mut();
    public uint len();
    public bool is_empty();
    @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public Cursor<K, V> lower_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public CursorMut<K, V, A> lower_bound_mut<Q>(Bound<Q> bound);
    @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public Cursor<K, V> upper_bound<Q>(Bound<Q> bound);
    @MutSelf @RustBorrowsSelf @RustBounds("K: Borrow, K: Ord") public CursorMut<K, V, A> upper_bound_mut<Q>(Bound<Q> bound);
}

/** An ordered set based on a B-Tree. */
@rust("std::collections::BTreeSet")
@RustClone
@RustCollection
public class BTreeSet<T, A> implements ToOwned {
    public BTreeSet();
    @RustBorrowsSelf @RustBounds("T: Borrow, T: Ord") public Range<T> range<K, R>(R range);
    @RustBorrowsSelf @RustBounds("T: Ord") public Difference<T, A> difference(&Set<T> other);
    @RustBorrowsSelf @RustBounds("T: Ord") public SymmetricDifference<T> symmetric_difference(&Set<T> other);
    @RustBorrowsSelf @RustBounds("T: Ord") public Intersection<T, A> intersection(&Set<T> other);
    @RustBorrowsSelf @RustBounds("T: Ord") public Union<T> union(&Set<T> other);
    @MutSelf @RustBounds("A: Clone") public void clear();
    @RustBounds("T: Borrow, T: Ord") public bool contains<Q>(&Q value);
    @RustRefOut @RustBounds("T: Borrow, T: Ord") public T? get<Q>(&Q value);
    @RustBounds("T: Ord") public bool is_disjoint(&Set<T> other);
    @RustBounds("T: Ord") public bool is_subset(&Set<T> other);
    @RustBounds("T: Ord") public bool is_superset(&Set<T> other);
    @RustRefOut @RustBounds("T: Ord") public T? first();
    @RustRefOut @RustBounds("T: Ord") public T? last();
    @MutSelf @RustBounds("T: Ord") public T? pop_first();
    @MutSelf @RustBounds("T: Ord") public T? pop_last();
    @MutSelf @RustBounds("T: Ord") public bool insert(T value);
    @MutSelf @RustBounds("T: Ord") public T? replace(T value);
    @MutSelf @RustBounds("T: Borrow, T: Ord") public bool remove<Q>(&Q value);
    @MutSelf @RustBounds("T: Borrow, T: Ord") public T? take<Q>(&Q value);
    @MutSelf @RustClosureRefs("0") @RustBounds("T: Ord") public void retain<F>((T) -> bool f);
    @MutSelf @RustBounds("T: Ord, A: Clone") public void append(&mut BTreeSet other);
    @MutSelf @RustBounds("T: Borrow, T: Ord, A: Clone") public BTreeSet split_off<Q>(&Q value);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") @RustBounds("T: Ord") public ExtractIf<T, R, F, A> extract_if<F, R>(R range, (T) -> bool pred);
    @RustBorrowsSelf public Iter<T> iter();
    public uint len();
    public bool is_empty();
}

/** A captured OS thread stack backtrace. */
@rust("std::backtrace::Backtrace")
public class Backtrace implements ToString {
    public static Backtrace capture();
    public static Backtrace force_capture();
    public static Backtrace disabled();
    public BacktraceStatus status();
}

/** The current status of a backtrace, indicating whether it was captured or */
@rust("std::backtrace::BacktraceStatus")
public enum BacktraceStatus {
    Unsupported, Disabled, Captured
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
    @MutSelf @RustBorrowsSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public T? pop();
    @MutSelf public void push(T item);
    public Vec<T> into_sorted_vec();
    @MutSelf public void append(&mut BinaryHeap other);
    @MutSelf @RustClosureRefs("0") public void retain<F>((T) -> bool f);
    @RustBorrowsSelf public Iter<T> iter();
    @RustRefOut public T? peek();
    public uint capacity();
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @RustRefOut public T[] as_slice();
    public Vec<T> into_vec();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain();
    @MutSelf public void clear();
}

/** A borrowed file descriptor. */
@rust("std::os::fd::BorrowedFd")
@RustClone
public class BorrowedFd implements AsFd, AsRawFd, IsTerminal, ToOwned {
    public static unsafe BorrowedFd borrow_raw(RawFd fd);
    public OwnedFd try_clone_to_owned() throws Error;
}

/** A borrowed handle. */
@rust("std::os::windows::io::BorrowedHandle")
@RustClone
public class BorrowedHandle implements AsHandle, AsRawHandle, IsTerminal, ToOwned {
    public static unsafe BorrowedHandle borrow_raw(RawHandle handle);
    public OwnedHandle try_clone_to_owned() throws Error;
}

/** A borrowed socket. */
@rust("std::os::windows::io::BorrowedSocket")
@RustClone
public class BorrowedSocket implements AsRawSocket, AsSocket, ToOwned {
    public static unsafe BorrowedSocket borrow_raw(RawSocket socket);
    public OwnedSocket try_clone_to_owned() throws Error;
}

/** An endpoint of a range of keys. */
@rust("std::ops::Bound")
@RustClone
public enum Bound<T> {
    Included(T), Excluded(T), Unbounded;

    @RustRefOut public Bound<T> as_ref();
    public Bound<U> map<U, F>((T) -> U f);
    public Bound<T> copied();
    @RustBounds("T: Clone") public Bound<T> cloned();
}

/** A pointer type that uniquely owns a heap allocation of type `T`. */
@rust("std::boxed::Box")
@RustClone
public class Box<T, A> implements RustIterator, ToOwned, ToString {
    public Box(T x);
    @RustDefault public Box();
    public T downcast<T>() throws Box;
    public unsafe T downcast_unchecked<T>();
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T x);
    public static U map<U>(Box this, (T) -> U f);
    public static (T, MaybeUninit<T>) take(Box boxed);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public T[] into_array() throws Box;
    public unsafe T assume_init();
    public static T write(Box boxed, T value);
    public static unsafe Box from_raw(T* raw);
    public static T* into_raw(Box b);
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
    public JoinHandle<T> spawn<F, T>(() -> T f) throws Error;
    public unsafe JoinHandle<T> spawn_unchecked<F, T>(() -> T f) throws Error;
    @RustBorrowsSelf public ScopedJoinHandle<T> spawn_scoped<F, T>(&Scope scope, () -> T f) throws Error;
}

/** An iterator over `u8` values of a reader. */
@rust("std::io::Bytes")
public class Bytes<R> implements RustIterator {
    @MutSelf public ubyte? next() throws Error;
}

/** A dynamically-sized view of a C string. */
@rust("std::ffi::CStr")
public class CStr {
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
    @RustRefOut public String to_str() throws Utf8Error;
}

/** A type representing an owned, C-compatible, nul-terminated string with no nul bytes in the */
@rust("std::ffi::c_str::CString")
@RustClone
@RustDerefs("CStr")
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
    @RustRefOut public String to_str() throws Utf8Error;
}

/** An iterator over the [`char`]s of a string slice, and their positions. */
@rust("std::str::CharIndices")
@RustClone
public class CharIndices implements RustIterator {
    @RustRefOut public String as_str();
    public uint offset();
    @MutSelf public (uint, char)? next();
}

/** An iterator over the [`char`]s of a string slice. */
@rust("std::str::Chars")
@RustClone
public class Chars implements RustIterator {
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
public class ChunkBy<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over slice in (non-overlapping) mutable chunks separated */
@rust("std::slice::ChunkByMut")
public class ChunkByMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::Chunks")
@RustClone
public class Chunks<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::ChunksExact")
@RustClone
public class ChunksExact<T> implements RustIterator {
    @RustRefOut public T[] remainder();
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::ChunksExactMut")
public class ChunksExactMut<T> implements RustIterator {
    @RustRefOut public T[] into_remainder();
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::ChunksMut")
public class ChunksMut<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator that clones the elements of an underlying iterator. */
@rust("std::iter::Cloned")
@RustClone
public class Cloned<I> implements RustIterator {
    @RustDefault public Cloned();
    @MutSelf public T? next();
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
}

/** An iterator over the command arguments. */
@rust("std::process::CommandArgs")
public class CommandArgs implements RustIterator {
    @MutSelf @RustRefOut public OsStr? next();
}

/** An iterator over the command environment variables. */
@rust("std::process::CommandEnvs")
public class CommandEnvs implements RustIterator {
    @MutSelf public (OsStr, OsStr?)? next();
}

/** Os-specific extensions for [`Command`] */
@rust("std::os::linux::process::CommandExt")
public interface CommandExt {
    @MutSelf @RustRefOut public Command create_pidfd(bool val);
}

/** An iterator over the fully resolved environment variables. */
@rust("std::process::CommandResolvedEnvs")
public class CommandResolvedEnvs implements RustIterator {
    @MutSelf public (OsString, OsString)? next();
}

/** A single component of a path. */
@rust("std::path::Component")
@RustClone
public enum Component implements ToOwned {
    Prefix(PrefixComponent), RootDir, CurDir, ParentDir, Normal(OsStr);

    @RustRefOut public OsStr as_os_str();
}

/** An iterator over the [`Component`]s of a [`Path`]. */
@rust("std::path::Components")
@RustClone
public class Components implements RustIterator, ToOwned {
    @RustRefOut public Path as_path();
    @MutSelf public Component? next();
}

/** A Condition Variable */
@rust("std::sync::Condvar")
public class Condvar {
    public Condvar();
    @RustBorrowsSelf public MutexGuard<T> wait<T>(MutexGuard<T> guard) throws PoisonError<T>;
    @RustBorrowsSelf @RustClosureRefs("1") public MutexGuard<T> wait_while<T, F>(MutexGuard<T> guard, (T) -> bool condition) throws PoisonError<T>;
    public (MutexGuard<T>, bool) wait_timeout_ms<T>(MutexGuard<T> guard, u32 ms) throws PoisonError<T>;
    public (MutexGuard<T>, WaitTimeoutResult) wait_timeout<T>(MutexGuard<T> guard, Duration dur) throws PoisonError<T>;
    @RustClosureRefs("2") public (MutexGuard<T>, WaitTimeoutResult) wait_timeout_while<T, F>(MutexGuard<T> guard, Duration dur, (T) -> bool condition) throws PoisonError<T>;
    public void notify_one();
    public void notify_all();
}

/** An iterator that copies the elements of an underlying iterator. */
@rust("std::iter::Copied")
@RustClone
public class Copied<I> implements RustIterator {
    @RustDefault public Copied();
    @MutSelf public T? next();
}

/** A clone-on-write smart pointer. */
@rust("std::borrow::Cow")
@RustClone
public enum Cow<B> implements ToOwned, ToString {
    Borrowed(B), Owned(B.Owned);

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
    @MutSelf @RustBounds("A: Clone") public LinkedList<T, A>? remove_current_as_list();
    @MutSelf @RustBounds("A: Clone") public LinkedList<T, A> split_after();
    @MutSelf @RustBounds("A: Clone") public LinkedList<T, A> split_before();
    @MutSelf public void push_front(T elt);
    @MutSelf public void push_back(T elt);
    @MutSelf public T? pop_front();
    @MutSelf public T? pop_back();
    @RustRefOut public T? front();
    @MutSelf @RustRefOut public T? front_mut();
    @RustRefOut public T? back();
    @MutSelf @RustRefOut public T? back_mut();
}

/** An iterator that repeats endlessly. */
@rust("std::iter::Cycle")
@RustClone
public class Cycle<I> implements RustIterator {
    @MutSelf public I.Item? next();
}

@rust("std::env::consts::DLL_EXTENSION")
public const String DLL_EXTENSION;

@rust("std::env::consts::DLL_PREFIX")
public const String DLL_PREFIX;

@rust("std::env::consts::DLL_SUFFIX")
public const String DLL_SUFFIX;

/** An iterator that decodes UTF-16 encoded code points from an iterator of `u16`s. */
@rust("std::char::DecodeUtf16")
@RustClone
public class DecodeUtf16<I> implements RustIterator {
    @MutSelf public char? next() throws DecodeUtf16Error;
}

/** The default [`Hasher`] used by [`RandomState`]. */
@rust("std::hash::DefaultHasher")
@RustClone
public class DefaultHasher implements ToOwned {
    public DefaultHasher();
}

/** A lazy iterator producing elements in the difference of `BTreeSet`s. */
@rust("std::collections::btree_set::Difference")
@RustClone
public class Difference<T, A> implements RustIterator, ToOwned {
    @MutSelf @RustRefOut public T? next();
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
public class Display implements ToString {
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Drain")
public class Drain<T, A> implements RustIterator {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::DrainSorted")
public class DrainSorted<T, A> implements RustIterator {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** A `Duration` type to represent a span of time, typically used for system */
@rust("std::time::Duration")
@RustClone
public class Duration {
    public Duration(ulong secs, u32 nanos);
    @RustDefault public Duration();
    public static Duration from_secs(ulong secs);
    public static Duration from_millis(ulong millis);
    public static Duration from_micros(ulong micros);
    public static Duration from_nanos(ulong nanos);
    public static Duration from_nanos_u128(u128 nanos);
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
    public static Duration from_secs_f64(double secs);
    public static Duration from_secs_f32(float secs);
    public Duration mul_f64(double rhs);
    public Duration mul_f32(float rhs);
    public Duration div_f64(double rhs);
    public Duration div_f32(float rhs);
    public double div_duration_f64(Duration rhs);
    public float div_duration_f32(Duration rhs);
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
public class Empty {
    @RustDefault public Empty();
}

/** An iterator of [`u16`] over the string encoded as UTF-16. */
@rust("std::str::EncodeUtf16")
@RustClone
public class EncodeUtf16 implements RustIterator {
    @MutSelf public ushort? next();
}

/** Iterator returned by [`OsStrExt::encode_wide`]. */
@rust("std::os::windows::ffi::EncodeWide")
@RustClone
public class EncodeWide implements RustIterator, ToOwned {
    @MutSelf public ushort? next();
}

/** A view into a single entry in a map, which may either be vacant or occupied. */
@rust("std::collections::btree_map::Entry")
public enum Entry<K, V, A> {
    Vacant(VacantEntry<K, V, A>), Occupied(OccupiedEntry<K, V, A>);

    @RustRefOut public V or_insert(V default);
    @RustRefOut public V or_insert_with<F>(() -> V default);
    @RustRefOut public V or_try_insert_with<F, E>(() -> Result<V, E> default) throws E;
    @RustRefOut @RustClosureRefs("0") public V or_insert_with_key<F>((K) -> V default);
    @RustRefOut @RustClosureRefs("0") public V or_try_insert_with_key<F, E>((K) -> Result<V, E> default) throws E;
    @RustRefOut public K key();
    @RustClosureRefs("0") public Entry and_modify<F>((V) -> void f);
    @RustBorrowsSelf public OccupiedEntry<K, V, A> insert_entry(V value);
    @RustRefOut public V or_default();
}

/** An iterator that yields the current count and the element during iteration. */
@rust("std::iter::Enumerate")
@RustClone
public class Enumerate<I> implements RustIterator {
    @RustDefault public Enumerate();
    @MutSelf public (uint, I.Item)? next();
}

/** The error type for I/O operations of the [`Read`], [`Write`], [`Seek`], and */
@rust("std::io::Error")
public class Error implements ToString {
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
public enum ErrorKind {
    NotFound, PermissionDenied, ConnectionRefused, ConnectionReset, HostUnreachable, NetworkUnreachable, ConnectionAborted, NotConnected, AddrInUse, AddrNotAvailable, NetworkDown, BrokenPipe, AlreadyExists, WouldBlock, NotADirectory, IsADirectory, DirectoryNotEmpty, ReadOnlyFilesystem, FilesystemLoop, StaleNetworkFileHandle, InvalidInput, InvalidData, TimedOut, WriteZero, StorageFull, NotSeekable, QuotaExceeded, FileTooLarge, ResourceBusy, ExecutableFileBusy, Deadlock, CrossesDevices, TooManyLinks, InvalidFilename, ArgumentListTooLong, Interrupted, Unsupported, UnexpectedEof, OutOfMemory, InProgress, Other
}

/** An iterator over the escaped version of a byte slice. */
@rust("std::slice::EscapeAscii")
@RustClone
public class EscapeAscii implements RustIterator {
    @MutSelf public ubyte? next();
}

/** This type represents the status code the current process can return */
@rust("std::process::ExitCode")
@RustClone
public class ExitCode implements ExitCodeExt, Termination, ToOwned {
    @RustDefault public ExitCode();
}

/** Describes the result of a process after it has terminated. */
@rust("std::process::ExitStatus")
@RustClone
public class ExitStatus implements ExitStatusExt, ToOwned, ToString {
    @RustDefault public ExitStatus();
    public bool success();
    public i32? code();
}

/** Unix-specific extensions to [`process::ExitStatus`] and */
@rust("std::os::unix::process::ExitStatusExt")
public interface ExitStatusExt {
    @RustStatic public Self from_raw(i32 raw);
    public i32? signal();
    public bool core_dumped();
    public i32? stopped_signal();
    public bool continued();
    public i32 into_raw();
}

/** This `struct` is created by the [`extract_if`] method on [`LinkedList`]. */
@rust("std::collections::ExtractIf")
public class ExtractIf<T, F, A> implements RustIterator {
    @MutSelf public T? next();
}

@rust("std::env::consts::FAMILY")
public const String FAMILY;

/** An object providing access to an open file on the filesystem. */
@rust("std::fs::File")
public class File implements AsFd, AsHandle, AsRawFd, AsRawHandle, FileExt, FromRawFd, FromRawHandle, IntoRawFd, IntoRawHandle, IsTerminal, Read, Seek, Write {
    public static File open<P>(P path) throws Error;
    public static File create<P>(P path) throws Error;
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
public class FileTimes implements FileTimesExt, ToOwned {
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
public class FileType implements FileTypeExt, ToOwned {
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

/** An iterator that filters the elements of `iter` with `predicate`. */
@rust("std::iter::Filter")
@RustClone
public class Filter<I, P> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** An iterator that uses `f` to both filter and map elements from `iter`. */
@rust("std::iter::FilterMap")
@RustClone
public class FilterMap<I, F> implements RustIterator {
    @MutSelf public B? next();
}

/** An iterator that maps each element to an iterator, and yields the elements */
@rust("std::iter::FlatMap")
@RustClone
public class FlatMap<I, U, F> implements RustIterator {
    @MutSelf public U.Item? next();
}

/** An iterator that flattens one level of nesting in an iterator of things */
@rust("std::iter::Flatten")
@RustClone
public class Flatten<I> implements RustIterator {
    @RustDefault public Flatten();
    @MutSelf public U.Item? next();
}

/** A classification of floating point numbers. */
@rust("std::num::FpCategory")
@RustClone
public enum FpCategory {
    Nan, Infinite, Zero, Subnormal, Normal
}

/** An error indicating that no nul byte was present. */
@rust("std::ffi::FromBytesUntilNulError")
@RustClone
public class FromBytesUntilNulError {
}

/** An error indicating that a nul byte was not in the expected position. */
@rust("std::ffi::FromBytesWithNulError")
@RustClone
public enum FromBytesWithNulError {
    InteriorNul, NotNulTerminated
}

/** Trait for types that can be converted from a fixed-size byte array with a specified endianness */
@rust("std::io::FromEndianBytes")
@RustImplementedBy("f32")
@RustImplementedBy("f64")
@RustImplementedBy("i128")
@RustImplementedBy("i16")
@RustImplementedBy("i32")
@RustImplementedBy("i64")
@RustImplementedBy("i8")
@RustImplementedBy("isize")
@RustImplementedBy("u128")
@RustImplementedBy("u16")
@RustImplementedBy("u32")
@RustImplementedBy("u64")
@RustImplementedBy("u8")
@RustImplementedBy("usize")
public interface FromEndianBytes {
}

/** A trait to express the ability to construct an object from a raw file */
@rust("std::os::fd::FromRawFd")
public interface FromRawFd {
    @RustStatic public unsafe Self from_raw_fd(RawFd fd);
}

/** Constructs I/O objects from raw handles. */
@rust("std::os::windows::io::FromRawHandle")
public interface FromRawHandle {
    @RustStatic public unsafe Self from_raw_handle(RawHandle handle);
}

/** Creates I/O objects from raw sockets. */
@rust("std::os::windows::io::FromRawSocket")
public interface FromRawSocket {
    @RustStatic public unsafe Self from_raw_socket(RawSocket sock);
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

/** An iterator that yields `None` forever after the underlying iterator */
@rust("std::iter::Fuse")
@RustClone
public class Fuse<I> implements RustIterator {
    @RustDefault public Fuse();
    @MutSelf public I.Item? next();
}

/** The error type returned by [`get_disjoint_mut`][`slice::get_disjoint_mut`]. */
@rust("std::slice::GetDisjointMutError")
@RustClone
public enum GetDisjointMutError {
    IndexOutOfBounds, OverlappingIndices
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
public class HashMap<K, V, S, A> implements ToOwned {
    public HashMap();
    public static Map<K, V> with_capacity(uint capacity);
    public static Map<K, V> with_hasher(S hash_builder);
    public static Map<K, V> with_capacity_and_hasher(uint capacity, S hasher);
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
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ExtractIf<K, V, F, A> extract_if<F>((K, V) -> bool pred);
    @MutSelf @RustClosureRefs("0") public void retain<F>((K, V) -> bool f);
    @MutSelf public void clear();
    @RustRefOut public S hasher();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf @RustBorrowsSelf public Entry<K, V, A> entry(K key);
    @RustRefOut @RustBounds("K: Borrow") public V? get<Q>(&Q k);
    @RustBounds("K: Borrow") public (K, V)? get_key_value<Q>(&Q k);
    @MutSelf @RustBounds("K: Borrow") public V?[] get_disjoint_mut<Q>(Q[] ks);
    @MutSelf @RustBounds("K: Borrow") public unsafe V?[] get_disjoint_unchecked_mut<Q>(Q[] ks);
    @RustBounds("K: Borrow") public bool contains_key<Q>(&Q k);
    @MutSelf @RustRefOut @RustBounds("K: Borrow") public V? get_mut<Q>(&Q k);
    @MutSelf public V? insert(K k, V v);
    @MutSelf @RustBorrowsSelf @RustRefOut public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf @RustBounds("K: Borrow") public V? remove<Q>(&Q k);
    @MutSelf @RustBounds("K: Borrow") public (K, V)? remove_entry<Q>(&Q k);
}

/** A [hash set] implemented as a `HashMap` where the value is `()`. */
@rust("std::collections::HashSet")
@RustClone
@RustCollection
public class HashSet<T, S, A> implements ToOwned {
    public HashSet();
    public static Set<T> with_capacity(uint capacity);
    public static Set<T> with_hasher(S hasher);
    public static Set<T> with_capacity_and_hasher(uint capacity, S hasher);
    public uint capacity();
    @RustBorrowsSelf public Iter<T> iter();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain();
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ExtractIf<T, F, A> extract_if<F>((T) -> bool pred);
    @MutSelf @RustClosureRefs("0") public void retain<F>((T) -> bool f);
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
    @RustBounds("T: Borrow") public bool contains<Q>(&Q value);
    @RustRefOut @RustBounds("T: Borrow") public T? get<Q>(&Q value);
    public bool is_disjoint(&Set<T> other);
    public bool is_subset(&Set<T> other);
    public bool is_superset(&Set<T> other);
    @MutSelf public bool insert(T value);
    @MutSelf public T? replace(T value);
    @MutSelf @RustBounds("T: Borrow") public bool remove<Q>(&Q value);
    @MutSelf @RustBounds("T: Borrow") public T? take<Q>(&Q value);
}

/** An iterator that infinitely [`accept`]s connections on a [`TcpListener`]. */
@rust("std::net::Incoming")
public class Incoming implements RustIterator {
    @MutSelf public TcpStream? next() throws Error;
}

/** An iterator that calls a function with a reference to each element before */
@rust("std::iter::Inspect")
@RustClone
public class Inspect<I, F> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** A measurement of a monotonically nondecreasing clock. */
@rust("std::time::Instant")
@RustClone
public class Instant implements ToOwned {
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
public enum IntErrorKind {
    Empty, InvalidDigit, PosOverflow, NegOverflow, Zero, NotAPowerOfTwo
}

/** A lazy iterator producing elements in the intersection of `BTreeSet`s. */
@rust("std::collections::btree_set::Intersection")
@RustClone
public class Intersection<T, A> implements RustIterator, ToOwned {
    @MutSelf @RustRefOut public T? next();
}

/** An error returned by [`BufWriter::into_inner`] which combines an error that */
@rust("std::io::IntoInnerError")
public class IntoInnerError<W> implements ToString {
    @RustRefOut public Error error();
    public W into_inner();
    public Error into_error();
    public (Error, W) into_parts();
}

/** An owning iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::IntoIter")
@RustClone
public class IntoIter<T, A> implements RustIterator, ToOwned {
    @RustDefault public IntoIter();
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

@rust("std::collections::IntoIterSorted")
@RustClone
public class IntoIterSorted<T, A> implements RustIterator, ToOwned {
    @RustRefOut public A allocator();
    @MutSelf public T? next();
}

/** An owning iterator over the keys of a `BTreeMap`. */
@rust("std::collections::btree_map::IntoKeys")
public class IntoKeys<K, V, A> implements RustIterator {
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
public class IntoValues<K, V, A> implements RustIterator {
    @RustDefault public IntoValues();
    @MutSelf public V? next();
}

/** This is the error type used by [`HandleOrInvalid`] when attempting to */
@rust("std::os::windows::io::InvalidHandleError")
@RustClone
public class InvalidHandleError implements ToOwned, ToString {
}

/** A buffer type used with `Write::write_vectored`. */
@rust("std::io::IoSlice")
@RustClone
@RustDerefs("[]")
public class IoSlice {
    public IoSlice(ubyte[] buf);
    @MutSelf public void advance(uint n);
    public static void advance_slices(&mut IoSlice[] bufs, uint n);
    @MutSelf @RustBounds("T: Ord") public void sort();
    @MutSelf @RustClosureRefs("0") public void sort_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_by_key<K, F>((T) -> K f);
    @MutSelf @RustClosureRefs("0") public void sort_by_cached_key<K, F>((T) -> K f);
    @RustBounds("T: Clone") public Vec<T> to_vec();
    @RustBounds("T: Copy") public Vec<T> repeat(uint n);
    public Self.Output concat<Item>();
    public Self.Output join<Separator>(Separator sep);
    public Self.Output connect<Separator>(Separator sep);
    public Vec<ubyte> to_ascii_uppercase();
    public Vec<ubyte> to_ascii_lowercase();
    @MutSelf public void sort_floats();
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
    @RustBorrowsSelf @RustClosureRefs("0") public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf @RustClosureRefs("0") public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    @RustBounds("T: PartialEq") public bool contains(&T x);
    @RustBounds("T: PartialEq") public bool starts_with(T[] needle);
    @RustBounds("T: PartialEq") public bool ends_with(T[] needle);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_suffix<P>(&P suffix);
    @RustBounds("T: Ord") public uint binary_search(&T x) throws Error;
    @RustClosureRefs("0") public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    @RustClosureRefs("1") public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf @RustBounds("T: Ord") public void sort_unstable();
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf @RustBounds("T: Ord") public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf @RustBounds("T: Clone") public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf @RustBounds("T: Clone") public void clone_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    @RustBounds("T: PartialOrd") public bool is_sorted();
    @RustClosureRefs("0") public bool is_sorted_by<F>((T, T) -> bool compare);
    @RustClosureRefs("0") public bool is_sorted_by_key<F, K>((T) -> K f);
    @RustClosureRefs("0") public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off<R>(R range);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public bool is_ascii();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut @RustBounds("T: Copy") public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
}

/** A buffer type used with `Read::read_vectored`. */
@rust("std::io::IoSliceMut")
@RustDerefs("[]")
public class IoSliceMut {
    public IoSliceMut(&mut ubyte[] buf);
    @MutSelf public void advance(uint n);
    public static void advance_slices(&mut IoSliceMut[] bufs, uint n);
    @MutSelf @RustBounds("T: Ord") public void sort();
    @MutSelf @RustClosureRefs("0") public void sort_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_by_key<K, F>((T) -> K f);
    @MutSelf @RustClosureRefs("0") public void sort_by_cached_key<K, F>((T) -> K f);
    @RustBounds("T: Clone") public Vec<T> to_vec();
    @RustBounds("T: Copy") public Vec<T> repeat(uint n);
    public Self.Output concat<Item>();
    public Self.Output join<Separator>(Separator sep);
    public Self.Output connect<Separator>(Separator sep);
    public Vec<ubyte> to_ascii_uppercase();
    public Vec<ubyte> to_ascii_lowercase();
    @MutSelf public void sort_floats();
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
    @RustBorrowsSelf @RustClosureRefs("0") public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf @RustClosureRefs("0") public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    @RustBounds("T: PartialEq") public bool contains(&T x);
    @RustBounds("T: PartialEq") public bool starts_with(T[] needle);
    @RustBounds("T: PartialEq") public bool ends_with(T[] needle);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_suffix<P>(&P suffix);
    @RustBounds("T: Ord") public uint binary_search(&T x) throws Error;
    @RustClosureRefs("0") public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    @RustClosureRefs("1") public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf @RustBounds("T: Ord") public void sort_unstable();
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf @RustBounds("T: Ord") public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf @RustBounds("T: Clone") public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf @RustBounds("T: Clone") public void clone_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    @RustBounds("T: PartialOrd") public bool is_sorted();
    @RustClosureRefs("0") public bool is_sorted_by<F>((T, T) -> bool compare);
    @RustClosureRefs("0") public bool is_sorted_by_key<F, K>((T) -> K f);
    @RustClosureRefs("0") public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off<R>(R range);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
    public bool is_ascii();
    public bool eq_ignore_ascii_case(ubyte[] other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    @RustBorrowsSelf public EscapeAscii escape_ascii();
    @RustRefOut public ubyte[] trim_ascii_start();
    @RustRefOut public ubyte[] trim_ascii_end();
    @RustRefOut public ubyte[] trim_ascii();
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut @RustBounds("T: Copy") public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
}

/** An IP address, either IPv4 or IPv6. */
@rust("std::net::IpAddr")
@RustClone
public enum IpAddr {
    V4(Ipv4Addr), V6(Ipv6Addr);

    public bool is_unspecified();
    public bool is_loopback();
    public bool is_multicast();
    public bool is_ipv4();
    public bool is_ipv6();
    public IpAddr to_canonical();
}

/** An IPv4 address. */
@rust("std::net::Ipv4Addr")
@RustClone
public class Ipv4Addr {
    public Ipv4Addr(ubyte a, ubyte b, ubyte c, ubyte d);
    public u32 to_bits();
    public static Ipv4Addr from_bits(u32 bits);
    public ubyte[4] octets();
    public static Ipv4Addr from_octets(ubyte[4] octets);
    public bool is_unspecified();
    public bool is_loopback();
    public bool is_private();
    public bool is_link_local();
    public bool is_multicast();
    public bool is_broadcast();
    public bool is_documentation();
    public Ipv6Addr to_ipv6_compatible();
    public Ipv6Addr to_ipv6_mapped();
}

/** An IPv6 address. */
@rust("std::net::Ipv6Addr")
@RustClone
public class Ipv6Addr {
    public Ipv6Addr(ushort a, ushort b, ushort c, ushort d, ushort e, ushort f, ushort g, ushort h);
    public u128 to_bits();
    public static Ipv6Addr from_bits(u128 bits);
    public ushort[8] segments();
    public static Ipv6Addr from_segments(ushort[8] segments);
    public bool is_unspecified();
    public bool is_loopback();
    public bool is_unique_local();
    public bool is_unicast_link_local();
    public bool is_multicast();
    public Ipv4Addr? to_ipv4_mapped();
    public Ipv4Addr? to_ipv4();
    public IpAddr to_canonical();
    public ubyte[16] octets();
    public static Ipv6Addr from_octets(ubyte[16] octets);
}

/** Trait to determine if a descriptor/handle refers to a terminal/tty. */
@rust("std::io::IsTerminal")
public interface IsTerminal {
    public bool is_terminal();
}

/** An iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Iter")
@RustClone
public class Iter<T> implements RustIterator, ToOwned {
    @RustDefault public Iter();
    @MutSelf @RustRefOut public T? next();
}

/** A mutable iterator over the elements of a `LinkedList`. */
@rust("std::collections::IterMut")
public class IterMut<T> implements RustIterator {
    @RustDefault public IterMut();
    @MutSelf @RustRefOut public T? next();
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
public class JoinPathsError implements ToString {
}

/** An iterator over the keys of a `BTreeMap`. */
@rust("std::collections::btree_map::Keys")
@RustClone
public class Keys<K, V> implements RustIterator, ToOwned {
    @RustDefault public Keys();
    @MutSelf @RustRefOut public K? next();
}

/** Layout of a block of memory. */
@rust("std::alloc::Layout")
@RustClone
public class Layout {
    public Layout();
    public static Layout from_size_align(uint size, uint align) throws LayoutError;
    public static unsafe Layout from_size_align_unchecked(uint size, uint align);
    public uint size();
    public uint align();
    public static Layout for_value<T>(&T t);
    public NonNull<ubyte> dangling_ptr();
    public Layout align_to(uint align) throws LayoutError;
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
public class Lines<B> implements RustIterator {
    @MutSelf public String? next() throws Error;
}

/** Created with the method [`lines_any`]. */
@rust("std::str::LinesAny")
@RustClone
public class LinesAny implements RustIterator {
    @MutSelf @RustRefOut public String? next();
}

/** A doubly-linked list with owned nodes. */
@rust("std::collections::LinkedList")
@RustClone
@RustCollection
public class LinkedList<T, A> implements ToOwned {
    public LinkedList();
    @MutSelf public void append(&mut LinkedList other);
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    public bool is_empty();
    public uint len();
    @MutSelf public void clear();
    @RustBounds("T: PartialEq") public bool contains(&T x);
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
    @MutSelf @RustBounds("A: Clone") public LinkedList<T, A> split_off(uint at);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ExtractIf<T, F, A> extract_if<F>((T) -> bool filter);
}

/** A thread local storage (TLS) key which owns its contents. */
@rust("std::thread::LocalKey")
public class LocalKey<T> {
    @RustClosureRefs("0") public R with<F, R>((T) -> R f);
    @RustClosureRefs("0") public R try_with<F, R>((T) -> R f) throws AccessError;
    public void set(T value);
    @RustBounds("T: Copy") public T get();
    @RustBounds("T: Default") public T take();
    public T replace(T value);
    @RustBounds("T: Copy") public void update((T) -> T f);
    @RustClosureRefs("0") public R with_borrow<F, R>((T) -> R f);
    @RustClosureRefs("0") public R with_borrow_mut<F, R>((T) -> R f);
}

/** A struct containing information about the location of a panic. */
@rust("std::panic::Location")
@RustClone
public class Location {
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
public class Map<I, F> implements RustIterator {
    @MutSelf public B? next();
}

/** An iterator that only accepts elements while `predicate` returns `Some(_)`. */
@rust("std::iter::MapWhile")
@RustClone
public class MapWhile<I, P> implements RustIterator {
    @MutSelf public B? next();
}

/** Created with the method [`match_indices`]. */
@rust("std::str::MatchIndices")
@RustClone
public class MatchIndices<P> implements RustIterator {
    @MutSelf public (uint, String)? next();
}

/** Created with the method [`matches`]. */
@rust("std::str::Matches")
@RustClone
public class Matches<P> implements RustIterator {
    @MutSelf @RustRefOut public String? next();
}

/** This struct is used to iterate through the control messages. */
@rust("std::os::unix::net::Messages")
public class Messages implements RustIterator {
    @MutSelf public AncillaryData? next() throws AncillaryError;
}

/** Metadata information about a file. */
@rust("std::fs::Metadata")
@RustClone
public class Metadata implements MetadataExt, ToOwned {
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
    @RustBorrowsSelf public MutexGuard<T> lock() throws PoisonError<T>;
    @RustBorrowsSelf public MutexGuard<T> try_lock() throws TryLockError<Guard>;
    public bool is_poisoned();
    public void clear_poison();
    public T into_inner() throws PoisonError<T>;
    @MutSelf @RustRefOut public T get_mut() throws PoisonError<T>;
}

/** An RAII implementation of a "scoped lock" of a mutex. When this structure is */
@rust("std::sync::MutexGuard")
public class MutexGuard<T> implements ToString {
}

/** `*mut T` but non-zero and [covariant]. */
@rust("std::ptr::NonNull")
@RustClone
public class NonNull<T> {
    public static NonNull without_provenance(NonZero<uint> addr);
    public static NonNull dangling();
    public static NonNull with_exposed_provenance(NonZero<uint> addr);
    public static unsafe NonNull new_unchecked(T* ptr);
    public static NonNull? new(T* ptr);
    public static NonNull from_ref(&T r);
    public static NonNull from_mut(&mut T r);
    public NonZero<uint> addr();
    public NonZero<uint> expose_provenance();
    public NonNull with_addr(NonZero<uint> addr);
    public NonNull map_addr((NonZero<uint>) -> NonZero<uint> f);
    public T* as_ptr();
    @RustRefOut public unsafe T as_ref();
    @MutSelf @RustRefOut public unsafe T as_mut();
    public NonNull<U> cast<U>();
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
public class NonZero<T> {
    public static NonZero? new(T n);
    public static unsafe NonZero new_unchecked(T n);
    public T get();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public static NonZero from_be(NonZero x);
    public static NonZero from_le(NonZero x);
    public NonZero? checked_add(ubyte other);
    public NonZero saturating_add(ubyte other);
    public NonZero? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZero midpoint(NonZero rhs);
    public bool is_power_of_two();
    public NonZero isqrt();
    public NonZero<byte> cast_signed();
    public NonZero? checked_mul(NonZero other);
    public NonZero saturating_mul(NonZero other);
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
    public NonZero<u32> count_ones();
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
    public NonZeroI128? checked_pow(u32 other);
    public NonZeroI128 saturating_pow(u32 other);
}

/** An [`i16`] that is known not to equal zero. */
@rust("std::num::NonZeroI16")
public class NonZeroI16 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
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
    public NonZeroI16? checked_pow(u32 other);
    public NonZeroI16 saturating_pow(u32 other);
}

/** An [`i32`] that is known not to equal zero. */
@rust("std::num::NonZeroI32")
public class NonZeroI32 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
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
    public NonZeroI32? checked_pow(u32 other);
    public NonZeroI32 saturating_pow(u32 other);
}

/** An [`i64`] that is known not to equal zero. */
@rust("std::num::NonZeroI64")
public class NonZeroI64 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
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
    public NonZeroI64? checked_pow(u32 other);
    public NonZeroI64 saturating_pow(u32 other);
}

/** An [`i8`] that is known not to equal zero. */
@rust("std::num::NonZeroI8")
public class NonZeroI8 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
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
    public NonZeroI8? checked_pow(u32 other);
    public NonZeroI8 saturating_pow(u32 other);
}

/** An [`isize`] that is known not to equal zero. */
@rust("std::num::NonZeroIsize")
public class NonZeroIsize {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
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
    public NonZeroIsize? checked_pow(u32 other);
    public NonZeroIsize saturating_pow(u32 other);
}

/** A [`u128`] that is known not to equal zero. */
@rust("std::num::NonZeroU128")
public class NonZeroU128 {
    public NonZeroU128 div_ceil(NonZeroU128 rhs);
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroU128? checked_add(u128 other);
    public NonZeroU128 saturating_add(u128 other);
    public NonZeroU128? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU128 midpoint(NonZeroU128 rhs);
    public bool is_power_of_two();
    public NonZeroU128 isqrt();
    public NonZero<i128> cast_signed();
    public NonZeroU128? checked_mul(NonZeroU128 other);
    public NonZeroU128 saturating_mul(NonZeroU128 other);
    public NonZeroU128? checked_pow(u32 other);
    public NonZeroU128 saturating_pow(u32 other);
}

/** A [`u16`] that is known not to equal zero. */
@rust("std::num::NonZeroU16")
public class NonZeroU16 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroU16? checked_add(ushort other);
    public NonZeroU16 saturating_add(ushort other);
    public NonZeroU16? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU16 midpoint(NonZeroU16 rhs);
    public bool is_power_of_two();
    public NonZeroU16 isqrt();
    public NonZero<short> cast_signed();
    public NonZeroU16? checked_mul(NonZeroU16 other);
    public NonZeroU16 saturating_mul(NonZeroU16 other);
    public NonZeroU16? checked_pow(u32 other);
    public NonZeroU16 saturating_pow(u32 other);
    public NonZeroU16 div_ceil(NonZeroU16 rhs);
}

/** A [`u32`] that is known not to equal zero. */
@rust("std::num::NonZeroU32")
public class NonZeroU32 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroU32? checked_add(u32 other);
    public NonZeroU32 saturating_add(u32 other);
    public NonZeroU32? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU32 midpoint(NonZeroU32 rhs);
    public bool is_power_of_two();
    public NonZeroU32 isqrt();
    public NonZero<i32> cast_signed();
    public NonZeroU32? checked_mul(NonZeroU32 other);
    public NonZeroU32 saturating_mul(NonZeroU32 other);
    public NonZeroU32? checked_pow(u32 other);
    public NonZeroU32 saturating_pow(u32 other);
    public NonZeroU32 div_ceil(NonZeroU32 rhs);
}

/** A [`u64`] that is known not to equal zero. */
@rust("std::num::NonZeroU64")
public class NonZeroU64 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroU64? checked_add(ulong other);
    public NonZeroU64 saturating_add(ulong other);
    public NonZeroU64? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU64 midpoint(NonZeroU64 rhs);
    public bool is_power_of_two();
    public NonZeroU64 isqrt();
    public NonZero<long> cast_signed();
    public NonZeroU64? checked_mul(NonZeroU64 other);
    public NonZeroU64 saturating_mul(NonZeroU64 other);
    public NonZeroU64? checked_pow(u32 other);
    public NonZeroU64 saturating_pow(u32 other);
    public NonZeroU64 div_ceil(NonZeroU64 rhs);
}

/** A [`u8`] that is known not to equal zero. */
@rust("std::num::NonZeroU8")
public class NonZeroU8 {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroU8? checked_add(ubyte other);
    public NonZeroU8 saturating_add(ubyte other);
    public NonZeroU8? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroU8 midpoint(NonZeroU8 rhs);
    public bool is_power_of_two();
    public NonZeroU8 isqrt();
    public NonZero<byte> cast_signed();
    public NonZeroU8? checked_mul(NonZeroU8 other);
    public NonZeroU8 saturating_mul(NonZeroU8 other);
    public NonZeroU8? checked_pow(u32 other);
    public NonZeroU8 saturating_pow(u32 other);
    public NonZeroU8 div_ceil(NonZeroU8 rhs);
}

/** A [`usize`] that is known not to equal zero. */
@rust("std::num::NonZeroUsize")
public class NonZeroUsize {
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public NonZero<u32> count_ones();
    public NonZeroUsize? checked_add(uint other);
    public NonZeroUsize saturating_add(uint other);
    public NonZeroUsize? checked_next_power_of_two();
    public u32 ilog2();
    public u32 ilog10();
    public NonZeroUsize midpoint(NonZeroUsize rhs);
    public bool is_power_of_two();
    public NonZeroUsize isqrt();
    public NonZero<int> cast_signed();
    public NonZeroUsize? checked_mul(NonZeroUsize other);
    public NonZeroUsize saturating_mul(NonZeroUsize other);
    public NonZeroUsize? checked_pow(u32 other);
    public NonZeroUsize saturating_pow(u32 other);
    public NonZeroUsize div_ceil(NonZeroUsize rhs);
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
public class NullHandleError implements ToOwned, ToString {
}

/** A buffer wrapper of which the internal size is based on the maximum */
@rust("std::fmt::NumBuffer")
public class NumBuffer<T> {
    public NumBuffer();
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

/** A low-level synchronization primitive for one-time global execution. */
@rust("std::sync::Once")
public class Once {
    public Once();
    public void call_once<F>(() -> void f);
    @RustClosureRefs("0") public void call_once_force<F>((OnceState) -> void f);
    public bool is_completed();
    public void wait();
    public void wait_force();
}

/** A synchronization primitive which can nominally be written to only once. */
@rust("std::sync::OnceLock")
@RustClone
public class OnceLock<T> implements ToOwned {
    public OnceLock();
    @RustRefOut public T? get();
    @MutSelf @RustRefOut public T? get_mut();
    @RustRefOut public T wait();
    public void set(T value) throws T;
    @RustRefOut public T get_or_init<F>(() -> T f);
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
public class OpenOptions implements OpenOptionsExt, OpenOptionsExt2, ToOwned {
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

/** An `Ordering` is the result of a comparison between two values. */
@rust("std::cmp::Ordering")
@RustClone
public enum Ordering {
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
public class OsStr implements OsStrExt, ToOwned {
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
    @MutSelf public void make_ascii_lowercase();
    @MutSelf public void make_ascii_uppercase();
    public OsString to_ascii_lowercase();
    public OsString to_ascii_uppercase();
    public bool is_ascii();
    public bool eq_ignore_ascii_case<S>(S other);
    @RustBorrowsSelf public Display display();
}

/** Platform-specific extensions to [`OsStr`]. */
@rust("std::os::unix::ffi::OsStrExt")
public interface OsStrExt {
    @RustStatic @RustRefOut public Self from_bytes(ubyte[] slice);
    @RustRefOut public ubyte[] as_bytes();
}

/** A type that can represent owned, mutable platform-native strings, but is */
@rust("std::ffi::os_str::OsString")
@RustClone
@RustCollection
@RustDerefs("OsStr")
public class OsString implements OsStringExt, ToOwned, Write {
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
    @RustRefOut public String? to_str();
    @RustBorrowsSelf public Cow<String> to_string_lossy();
    public OsString to_os_string();
    public bool is_empty();
    public uint len();
    public OsString into_os_string();
    public (OsStr, OsStr) split_at(uint mid);
    public (OsStr, OsStr)? split_at_checked(uint mid);
    @RustRefOut public ubyte[] as_encoded_bytes();
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
    @RustStatic public Self from_vec(Vec<ubyte> vec);
    public Vec<ubyte> into_vec();
}

/** The output of a finished process. */
@rust("std::process::Output")
@RustClone
public class Output implements ToOwned {
    public ExitStatus status;
    public Vec<ubyte> stdout;
    public Vec<ubyte> stderr;
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
public class PanicHookInfo implements ToString {
    @RustRefOut public Any payload();
    @RustRefOut public String? payload_as_str();
    @RustRefOut public Location? location();
}

/** An error which can be returned when parsing a float. */
@rust("std::num::ParseFloatError")
@RustClone
public class ParseFloatError {
}

/** An error which can be returned when parsing an integer. */
@rust("std::num::ParseIntError")
@RustClone
public class ParseIntError {
    @RustRefOut public IntErrorKind kind();
}

/** A slice of a path (akin to [`str`]). */
@rust("std::path::Path")
@RustClone
@RustOwnedAs("PathBuf")
public class Path implements ToOwned {
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
    public bool starts_with<P>(P base);
    public bool ends_with<P>(P child);
    @RustRefOut public OsStr? file_stem();
    @RustRefOut public OsStr? file_prefix();
    @RustRefOut public OsStr? extension();
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
@RustDerefs("Path")
public class PathBuf implements ToOwned {
    public PathBuf();
    public static PathBuf with_capacity(uint capacity);
    @RustRefOut public Path as_path();
    @RustRefOut public Path leak();
    @MutSelf public void push<P>(P path);
    @MutSelf public bool pop();
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
    public bool starts_with<P>(P base);
    public bool ends_with<P>(P child);
    @RustRefOut public OsStr? file_stem();
    @RustRefOut public OsStr? file_prefix();
    @RustRefOut public OsStr? extension();
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

/** An iterator with a `peek()` that returns an optional reference to the next */
@rust("std::iter::Peekable")
@RustClone
public class Peekable<I> implements RustIterator {
    @MutSelf @RustRefOut public I.Item? peek();
    @MutSelf @RustRefOut public I.Item? peek_mut();
    @MutSelf @RustClosureRefs("0") public I.Item? next_if((I.Item) -> bool func);
    @MutSelf public I.Item? next_if_eq<T>(&T expected);
    @MutSelf public R? next_if_map<R>((I.Item) -> Result<R, I.Item> f);
    @MutSelf @RustClosureRefs("0") public R? next_if_map_mut<R>((I.Item) -> R? f);
    @MutSelf public I.Item? next();
}

/** Representation of the various permissions on a file. */
@rust("std::fs::Permissions")
@RustClone
public class Permissions implements PermissionsExt, ToOwned {
    public bool readonly();
    @MutSelf public void set_readonly(bool readonly);
}

/** Unix-specific extensions to [`fs::Permissions`]. */
@rust("std::os::unix::fs::PermissionsExt")
public interface PermissionsExt {
    public u32 mode();
    @MutSelf public void set_mode(u32 mode);
    @RustStatic public Self from_mode(u32 mode);
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
public class Pin<Ptr> {
    public Pin(Ptr pointer);
    public static Ptr into_inner(Pin<Ptr> pin);
    public static unsafe Pin<Ptr> new_unchecked(Ptr pointer);
    @RustRefOut @RustBounds("Ptr: Deref") public Pin<Ptr.Target> as_ref();
    @MutSelf @RustRefOut @RustBounds("Ptr: DerefMut") public Pin<Ptr.Target> as_mut();
    @RustRefOut @RustBounds("Ptr: DerefMut") public Pin<Ptr.Target> as_deref_mut();
    @MutSelf public void set(Ptr.Target value);
    public static unsafe Ptr into_inner_unchecked(Pin<Ptr> pin);
    @RustRefOut @RustClosureRefs("0") public unsafe Pin<U> map_unchecked<U, F>((T) -> U func);
    @RustRefOut public T get_ref();
    @RustRefOut public Pin<T> into_ref();
    @RustRefOut @RustBounds("T: Unpin") public T get_mut();
    @RustRefOut public unsafe T get_unchecked_mut();
    @RustRefOut @RustClosureRefs("0") public unsafe Pin<U> map_unchecked_mut<U, F>((T) -> U func);
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

/** Windows path prefixes, e.g., `C:` or `\\server\share`. */
@rust("std::path::Prefix")
@RustClone
public enum Prefix implements ToOwned {
    Verbatim(OsStr), VerbatimUNC(OsStr, OsStr), VerbatimDisk(ubyte), DeviceNS(OsStr), UNC(OsStr, OsStr), Disk(ubyte);

    public bool is_verbatim();
}

/** A structure wrapping a Windows path prefix as well as its unparsed string */
@rust("std::path::PrefixComponent")
@RustClone
public class PrefixComponent implements ToOwned {
    @RustBorrowsSelf public Prefix kind();
    @RustRefOut public OsStr as_os_str();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::RChunks")
@RustClone
public class RChunks<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) chunks (`chunk_size` elements at a */
@rust("std::slice::RChunksExact")
@RustClone
public class RChunksExact<T> implements RustIterator {
    @RustRefOut public T[] remainder();
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::RChunksExactMut")
public class RChunksExactMut<T> implements RustIterator {
    @RustRefOut public T[] into_remainder();
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over a slice in (non-overlapping) mutable chunks (`chunk_size` */
@rust("std::slice::RChunksMut")
public class RChunksMut<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** Created with the method [`rmatch_indices`]. */
@rust("std::str::RMatchIndices")
@RustClone
public class RMatchIndices<P> implements RustIterator {
    @MutSelf public (uint, String)? next();
}

/** Created with the method [`rmatches`]. */
@rust("std::str::RMatches")
@RustClone
public class RMatches<P> implements RustIterator {
    @MutSelf @RustRefOut public String? next();
}

/** An iterator over the subslices of the vector which are separated */
@rust("std::slice::RSplitMut")
public class RSplitMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over subslices separated by elements that match a */
@rust("std::slice::RSplitNMut")
public class RSplitNMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** Created with the method [`rsplit_terminator`]. */
@rust("std::str::RSplitTerminator")
@RustClone
public class RSplitTerminator<P> implements RustIterator {
    @RustRefOut public String? remainder();
    @MutSelf @RustRefOut public String? next();
}

/** `RandomState` is the default state for [`HashMap`] types. */
@rust("std::hash::RandomState")
@RustClone
public class RandomState implements ToOwned {
    public RandomState();
}

/** An iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::btree_map::Range")
@RustClone
public class Range<K, V> implements RustIterator, ToOwned {
    @RustDefault public Range();
    @MutSelf public (K, V)? next();
}

/** A mutable iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::btree_map::RangeMut")
public class RangeMut<K, V> implements RustIterator {
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
    @RustClosureRefs("0") public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T value);
    public static T try_unwrap(Rc this) throws Rc;
    public static T? into_inner(Rc this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public T[] into_array() throws Rc;
    public unsafe T assume_init();
    public static unsafe Rc from_raw(T* ptr);
    public static T* into_raw(Rc this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    public static T* as_ptr(&Rc this);
    @RustBounds("A: Clone") public static Weak<T, A> downgrade(&Rc this);
    public static uint weak_count(&Rc this);
    public static uint strong_count(&Rc this);
    @RustRefOut public static T? get_mut(&mut Rc this);
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
public class ReadDir implements RustIterator {
    @MutSelf public DirEntry? next() throws Error;
}

/** An error returned from the [`recv`] function on a [`Receiver`]. */
@rust("std::sync::mpsc::RecvError")
@RustClone
public class RecvError implements ToOwned, ToString {
}

/** This enumeration is the list of possible errors that made [`recv_timeout`] */
@rust("std::sync::mpsc::RecvTimeoutError")
@RustClone
public enum RecvTimeoutError implements ToOwned, ToString {
    Timeout, Disconnected
}

/** A reader which yields one byte over and over and over and over and over and... */
@rust("std::io::Repeat")
public class Repeat {
}

/** A double-ended iterator with the direction inverted. */
@rust("std::iter::Rev")
@RustClone
public class Rev<T> implements RustIterator {
    @RustDefault public Rev();
    @MutSelf public I.Item? next();
}

/** A trait for dealing with iterators. */
@rust("std::iter::Iterator")
public interface RustIterator {
    @MutSelf public Self.Item? next();
    @MutSelf public Self.Item[] next_chunk() throws IntoIter<Self.Item>;
    public (uint, uint?) size_hint();
    public uint count();
    public Self.Item? last();
    @MutSelf public void advance_by(uint n) throws NonZero<uint>;
    @MutSelf public Self.Item? nth(uint n);
    public StepBy<Self> step_by(uint step);
    public Chain<Self, U.IntoIter> chain<U>(U other);
    public Zip<Self, U.IntoIter> zip<U>(U other);
    public Intersperse<Self> intersperse(Self.Item separator);
    public IntersperseWith<Self, G> intersperse_with<G>(() -> Self.Item separator);
    public Map<Self, F> map<B, F>((Self.Item) -> B f);
    public void for_each<F>((Self.Item) -> void f);
    @RustClosureRefs("0") public Filter<Self, P> filter<P>((Self.Item) -> bool predicate);
    public FilterMap<Self, F> filter_map<B, F>((Self.Item) -> B? f);
    public Enumerate<Self> enumerate();
    public Peekable<Self> peekable();
    @RustClosureRefs("0") public SkipWhile<Self, P> skip_while<P>((Self.Item) -> bool predicate);
    @RustClosureRefs("0") public TakeWhile<Self, P> take_while<P>((Self.Item) -> bool predicate);
    public MapWhile<Self, P> map_while<B, P>((Self.Item) -> B? predicate);
    public Skip<Self> skip(uint n);
    public Take<Self> take(uint n);
    @RustClosureRefs("1") public Scan<Self, St, F> scan<St, B, F>(St initial_state, (St, Self.Item) -> B? f);
    public FlatMap<Self, U, F> flat_map<U, F>((Self.Item) -> U f);
    public Flatten<Self> flatten();
    @RustClosureRefs("0") public MapWindows<Self, F> map_windows<F, R>((Self.Item[]) -> R f);
    public Fuse<Self> fuse();
    @RustClosureRefs("0") public Inspect<Self, F> inspect<F>((Self.Item) -> void f);
    @MutSelf @RustRefOut public Self by_ref();
    public B collect<B>();
    @MutSelf public Self.TryType try_collect<B>();
    @RustRefOut public E collect_into<E>(&mut E collection);
    @RustClosureRefs("0") public (B, B) partition<B, F>((Self.Item) -> bool f);
    @RustClosureRefs("0") public uint partition_in_place<T, P>((T) -> bool predicate);
    public bool is_partitioned<P>((Self.Item) -> bool predicate);
    @MutSelf public R try_fold<B, F, R>(B init, (B, Self.Item) -> R f);
    @MutSelf public R try_for_each<F, R>((Self.Item) -> R f);
    public B fold<B, F>(B init, (B, Self.Item) -> B f);
    public Self.Item? reduce<F>((Self.Item, Self.Item) -> Self.Item f);
    @MutSelf public Self.TryType try_reduce<R>((Self.Item, Self.Item) -> R f);
    @MutSelf public bool all<F>((Self.Item) -> bool f);
    @MutSelf public bool any<F>((Self.Item) -> bool f);
    @MutSelf @RustClosureRefs("0") public Self.Item? find<P>((Self.Item) -> bool predicate);
    @MutSelf public B? find_map<B, F>((Self.Item) -> B? f);
    @MutSelf @RustClosureRefs("0") public Self.TryType try_find<R>((Self.Item) -> R f);
    @MutSelf public uint? position<P>((Self.Item) -> bool predicate);
    @MutSelf public uint? rposition<P>((Self.Item) -> bool predicate);
    public Self.Item? max();
    public Self.Item? min();
    @RustClosureRefs("0") public Self.Item? max_by_key<B, F>((Self.Item) -> B f);
    @RustClosureRefs("0") public Self.Item? max_by<F>((Self.Item, Self.Item) -> Ordering compare);
    @RustClosureRefs("0") public Self.Item? min_by_key<B, F>((Self.Item) -> B f);
    @RustClosureRefs("0") public Self.Item? min_by<F>((Self.Item, Self.Item) -> Ordering compare);
    public Rev<Self> rev();
    public (FromA, FromB) unzip<A, B, FromA, FromB>();
    public Copied<Self> copied<T>();
    public Cloned<Self> cloned<T>();
    public Cycle<Self> cycle();
    public ArrayChunks<Self> array_chunks();
    public S sum<S>();
    public P product<P>();
    public Ordering cmp<I>(I other);
    public Ordering cmp_by<I, F>(I other, (Self.Item, I.Item) -> Ordering cmp);
    public Ordering? partial_cmp<I>(I other);
    public Ordering? partial_cmp_by<I, F>(I other, (Self.Item, I.Item) -> Ordering? partial_cmp);
    public bool eq<I>(I other);
    public bool eq_by<I, F>(I other, (Self.Item, I.Item) -> bool eq);
    public bool ne<I>(I other);
    public bool lt<I>(I other);
    public bool le<I>(I other);
    public bool gt<I>(I other);
    public bool ge<I>(I other);
    public bool is_sorted();
    @RustClosureRefs("0") public bool is_sorted_by<F>((Self.Item, Self.Item) -> bool compare);
    public bool is_sorted_by_key<F, K>((Self.Item) -> K f);
}

/** A reader-writer lock */
@rust("std::sync::RwLock")
public class RwLock<T> {
    public RwLock(T t);
    @RustDefault public RwLock();
    @RustBorrowsSelf public RwLockReadGuard<T> read() throws PoisonError<T>;
    @RustBorrowsSelf public RwLockReadGuard<T> try_read() throws TryLockError<Guard>;
    @RustBorrowsSelf public RwLockWriteGuard<T> write() throws PoisonError<T>;
    @RustBorrowsSelf public RwLockWriteGuard<T> try_write() throws TryLockError<Guard>;
    public bool is_poisoned();
    public void clear_poison();
    public T into_inner() throws PoisonError<T>;
    @MutSelf @RustRefOut public T get_mut() throws PoisonError<T>;
}

/** RAII structure used to release the shared read access of a lock when */
@rust("std::sync::RwLockReadGuard")
public class RwLockReadGuard<T> implements ToString {
}

/** RAII structure used to release the exclusive write access of a lock when */
@rust("std::sync::RwLockWriteGuard")
public class RwLockWriteGuard<T> implements ToString {
    public static RwLockReadGuard<T> downgrade(RwLockWriteGuard s);
}

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
public class Saturating<T> {
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

/** An iterator to maintain state while iterating another iterator. */
@rust("std::iter::Scan")
@RustClone
public class Scan<I, St, F> implements RustIterator {
    @MutSelf public B? next();
}

@rust("std::os::unix::net::ScmCredentials")
public class ScmCredentials implements RustIterator {
    @MutSelf public SocketCred? next();
}

/** This control message contains file descriptors. */
@rust("std::os::unix::net::ScmRights")
public class ScmRights implements RustIterator {
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
public enum SeekFrom implements ToOwned {
    Start(ulong), End(long), Current(long)
}

/** An error returned from the [`Sender::send`] or [`SyncSender::send`] */
@rust("std::sync::mpsc::SendError")
@RustClone
public class SendError<T> implements ToOwned, ToString {
}

/** Possible values which can be passed to the [`TcpStream::shutdown`] method. */
@rust("std::net::Shutdown")
@RustClone
public enum Shutdown implements ToOwned {
    Read, Write, Both
}

/** A writer which will move data into the void. */
@rust("std::io::Sink")
@RustClone
public class Sink {
    @RustDefault public Sink();
}

/** An iterator that skips over `n` elements of `iter`. */
@rust("std::iter::Skip")
@RustClone
public class Skip<I> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** An iterator that rejects elements while `predicate` returns `true`. */
@rust("std::iter::SkipWhile")
@RustClone
public class SkipWhile<I, P> implements RustIterator {
    @MutSelf public I.Item? next();
}

@rust("std::sys::net::connection::socket::windows::Socket")
public class Socket {
}

/** An address associated with a Unix socket. */
@rust("std::os::unix::net::SocketAddr")
@RustClone
public class SocketAddr implements SocketAddrExt, ToOwned {
    public static SocketAddr from_pathname<P>(P path) throws Error;
    public bool is_unnamed();
    @RustRefOut public Path? as_pathname();
}

/** Platform-specific extensions to [`SocketAddr`]. */
@rust("std::os::linux::net::SocketAddrExt")
public interface SocketAddrExt {
    @RustStatic public SocketAddr from_abstract_name<N>(N name) throws Error;
    @RustRefOut public ubyte[]? as_abstract_name();
}

/** An IPv4 socket address. */
@rust("std::net::SocketAddrV4")
@RustClone
public class SocketAddrV4 {
    public SocketAddrV4(Ipv4Addr ip, ushort port);
    @RustRefOut public Ipv4Addr ip();
    @MutSelf public void set_ip(Ipv4Addr new_ip);
    public ushort port();
    @MutSelf public void set_port(ushort new_port);
}

/** An IPv6 socket address. */
@rust("std::net::SocketAddrV6")
@RustClone
public class SocketAddrV6 {
    public SocketAddrV6(Ipv6Addr ip, ushort port, u32 flowinfo, u32 scope_id);
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
public class SocketCred implements ToOwned {
}

/** A splicing iterator for `Vec`. */
@rust("std::vec::Splice")
public class Splice<I, A> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** An iterator over the contents of an instance of `BufRead` split on a */
@rust("std::io::Split")
public class Split<B> implements RustIterator {
    @MutSelf public Vec<ubyte>? next() throws Error;
}

/** An iterator over the non-ASCII-whitespace substrings of a string, */
@rust("std::str::SplitAsciiWhitespace")
@RustClone
public class SplitAsciiWhitespace implements RustIterator {
    @MutSelf @RustRefOut public String? next();
}

/** An iterator over the mutable subslices of the vector which are separated */
@rust("std::slice::SplitInclusiveMut")
public class SplitInclusiveMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over the mutable subslices of the vector which are separated */
@rust("std::slice::SplitMut")
public class SplitMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator over subslices separated by elements that match a predicate */
@rust("std::slice::SplitNMut")
public class SplitNMut<T, P> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** An iterator that splits an environment variable into paths according to */
@rust("std::env::SplitPaths")
public class SplitPaths implements RustIterator {
    @MutSelf public PathBuf? next();
}

/** Created with the method [`split_terminator`]. */
@rust("std::str::SplitTerminator")
@RustClone
public class SplitTerminator<P> implements RustIterator {
    @RustRefOut public String? remainder();
    @MutSelf @RustRefOut public String? next();
}

/** An iterator over the non-whitespace substrings of a string, */
@rust("std::str::SplitWhitespace")
@RustClone
public class SplitWhitespace implements RustIterator {
    @MutSelf @RustRefOut public String? next();
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

/** An iterator for stepping iterators by a custom amount. */
@rust("std::iter::StepBy")
@RustClone
public class StepBy<I> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** A UTF-8–encoded, growable string. */
@rust("std::string::String")
@RustClone
@RustCollection
@RustDerefs("str")
public class String implements ToOwned, ToString, Write {
    public String();
    public static String with_capacity(uint capacity);
    public static String from_utf8(Vec<ubyte> vec) throws FromUtf8Error;
    public static Cow<String> from_utf8_lossy(ubyte[] v);
    public static String from_utf16(ushort[] v) throws FromUtf16Error;
    public static String from_utf16_lossy(ushort[] v);
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
    @MutSelf public void retain<F>((char) -> bool f);
    @MutSelf public void insert(uint idx, char ch);
    @MutSelf public void insert_str(uint idx, &String string);
    @MutSelf @RustRefOut public unsafe Vec<ubyte> as_mut_vec();
    public uint len();
    public bool is_empty();
    @MutSelf public String split_off(uint at);
    @MutSelf public void clear();
    @MutSelf @RustBorrowsSelf public Drain drain<R>(R range);
    @MutSelf public void replace_range<R>(R range, &String replace_with);
    public String into_boxed_str();
    @RustRefOut public String leak();
    public String replace<P>(P from, &String to);
    public String replacen<P>(P pat, &String to, uint count);
    public String to_lowercase();
    public String to_uppercase();
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
    @RustRefOut public String trim_end_matches<P>(P pat);
    @RustRefOut public String trim_left_matches<P>(P pat);
    @RustRefOut public String trim_right_matches<P>(P pat);
    public F parse<F>() throws Error;
    public bool is_ascii();
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
    public ubyte[] into_boxed_bytes();
    public String word_to_titlecase();
    public String to_casefold_unnormalized();
    public String into_string();
}

/** An error returned from [`Path::strip_prefix`] if the prefix was not found. */
@rust("std::path::StripPrefixError")
@RustClone
public class StripPrefixError implements ToOwned, ToString {
}

/** A lazy iterator producing elements in the symmetric difference of `BTreeSet`s. */
@rust("std::collections::btree_set::SymmetricDifference")
@RustClone
public class SymmetricDifference<T> implements RustIterator, ToOwned {
    @MutSelf @RustRefOut public T? next();
}

/** The sending-half of Rust's synchronous [`sync_channel`] type. */
@rust("std::sync::mpsc::SyncSender")
@RustClone
public class SyncSender<T> implements ToOwned {
    public void send(T t) throws SendError<T>;
    public void try_send(T t) throws TrySendError<T>;
}

/** The default memory allocator provided by the operating system. */
@rust("std::alloc::System")
@RustClone
public class System implements ToOwned {
    @RustDefault public System();
}

/** The system random number generator. */
@rust("std::random::SystemRng")
@RustClone
public class SystemRng implements ToOwned {
    @RustDefault public SystemRng();
}

/** A measurement of the system clock, useful for talking to */
@rust("std::time::SystemTime")
@RustClone
public class SystemTime implements ToOwned {
    public static SystemTime now();
    public Duration duration_since(SystemTime earlier) throws SystemTimeError;
    public Duration elapsed() throws SystemTimeError;
    public SystemTime? checked_add(Duration duration);
    public SystemTime? checked_sub(Duration duration);
}

/** An error returned from the `duration_since` and `elapsed` methods on */
@rust("std::time::SystemTimeError")
@RustClone
public class SystemTimeError implements ToOwned, ToString {
    public Duration duration();
}

/** An iterator that only accepts elements while `predicate` returns `true`. */
@rust("std::iter::TakeWhile")
@RustClone
public class TakeWhile<I, P> implements RustIterator {
    @MutSelf public I.Item? next();
}

/** A TCP socket server, listening for connections. */
@rust("std::net::TcpListener")
public class TcpListener implements AsFd, AsRawFd, AsRawSocket, AsSocket, FromRawFd, FromRawSocket, IntoRawFd, IntoRawSocket {
    public static TcpListener bind<A>(A addr) throws Error;
    public SocketAddr local_addr() throws Error;
    public TcpListener try_clone() throws Error;
    public (TcpStream, SocketAddr) accept() throws Error;
    @RustBorrowsSelf public Incoming incoming();
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
@RustImplementedBy("never")
public interface Termination {
    public ExitCode report();
}

/** A handle to a thread. */
@rust("std::thread::Thread")
@RustClone
public class Thread implements ToOwned {
    public void unpark();
    public ThreadId id();
    @RustRefOut public String? name();
}

/** A unique identifier for a running thread. */
@rust("std::thread::ThreadId")
@RustClone
public class ThreadId implements ToOwned {
}

/** Returns an iterator that yields the case-folded equivalent of a `char`. */
@rust("std::char::ToCasefold")
@RustClone
public class ToCasefold implements RustIterator {
    @MutSelf public char? next();
}

/** Returns an iterator that yields the lowercase equivalent of a `char`. */
@rust("std::char::ToLowercase")
@RustClone
public class ToLowercase implements RustIterator {
    @MutSelf public char? next();
}

/** A generalization of `Clone` to borrowed data. */
@rust("std::borrow::ToOwned")
@RustBlanket("Clone")
@RustImplementedBy("[]")
@RustImplementedBy("str")
public interface ToOwned {
    public Self.Owned to_owned();
    public void clone_into(&mut Self.Owned target);
}

/** A trait for objects which can be converted or resolved to one or more */
@rust("std::net::ToSocketAddrs")
@RustImplementedBy("str")
public interface ToSocketAddrs {
    public Self.Iter to_socket_addrs() throws Error;
}

/** A trait for converting a value to a `String`. */
@rust("std::string::ToString")
@RustBlanket("Display")
public interface ToString {
    public String to_string();
}

/** Returns an iterator that yields the uppercase equivalent of a `char`. */
@rust("std::char::ToUppercase")
@RustClone
public class ToUppercase implements RustIterator {
    @MutSelf public char? next();
}

/** An error which can be returned when converting a floating-point value of seconds */
@rust("std::time::TryFromFloatSecsError")
@RustClone
public class TryFromFloatSecsError {
}

/** The error type returned when a checked integral type conversion fails. */
@rust("std::num::TryFromIntError")
@RustClone
public class TryFromIntError {
    @RustRefOut public IntErrorKind kind();
}

/** An enumeration of possible errors which can occur while trying to acquire a lock */
@rust("std::fs::TryLockError")
public enum TryLockError implements ToString {
    Error(Error), WouldBlock
}

/** This enumeration is the list of the possible reasons that [`try_recv`] could */
@rust("std::sync::mpsc::TryRecvError")
@RustClone
public enum TryRecvError implements ToOwned, ToString {
    Empty, Disconnected
}

/** The error type for `try_reserve` methods. */
@rust("std::collections::TryReserveError")
@RustClone
public class TryReserveError implements ToOwned, ToString {
}

/** This enumeration is the list of the possible error outcomes for the */
@rust("std::sync::mpsc::TrySendError")
@RustClone
public enum TrySendError<T> implements ToOwned, ToString {
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
public class Union<T> implements RustIterator, ToOwned {
    @MutSelf @RustRefOut public T? next();
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

/** An iterator used to decode a slice of mostly UTF-8 bytes to string slices */
@rust("std::str::Utf8Chunks")
@RustClone
public class Utf8Chunks implements RustIterator {
    @MutSelf public Utf8Chunk? next();
}

/** Errors which can occur when attempting to interpret a sequence of [`u8`] */
@rust("std::str::Utf8Error")
@RustClone
public class Utf8Error {
    public uint valid_up_to();
    public uint? error_len();
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
public class Values<K, V> implements RustIterator, ToOwned {
    @RustDefault public Values();
    @MutSelf @RustRefOut public V? next();
}

/** A mutable iterator over the values of a `BTreeMap`. */
@rust("std::collections::btree_map::ValuesMut")
public class ValuesMut<K, V> implements RustIterator {
    @RustDefault public ValuesMut();
    @MutSelf @RustRefOut public V? next();
}

/** The error type for operations interacting with environment variables. */
@rust("std::env::VarError")
@RustClone
public enum VarError implements ToOwned, ToString {
    NotPresent, NotUnicode(OsString)
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::Vars")
public class Vars implements RustIterator {
    @MutSelf public (String, String)? next();
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::VarsOs")
public class VarsOs implements RustIterator {
    @MutSelf public (OsString, OsString)? next();
}

/** A contiguous growable array type, written as `Vec<T>`, short for 'vector'. */
@rust("std::vec::Vec")
@RustClone
@RustCollection
@RustDerefs("[]")
public class Vec<T, A> implements ToOwned {
    public Vec();
    public static Vec with_capacity(uint capacity);
    public static unsafe Vec from_raw_parts(T* ptr, uint length, uint capacity);
    public (T*, uint, uint) into_raw_parts();
    @MutSelf public void push(T value);
    @MutSelf @RustRefOut public T push_mut(T value);
    public uint capacity();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    public T[] into_boxed_slice();
    public T[] into_array() throws Vec;
    @MutSelf public void truncate(uint len);
    @RustRefOut public T[] as_slice();
    @MutSelf @RustRefOut public T[] as_mut_slice();
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    @MutSelf public unsafe void set_len(uint new_len);
    @MutSelf public T swap_remove(uint index);
    @MutSelf public void insert(uint index, T element);
    @MutSelf @RustRefOut public T insert_mut(uint index, T element);
    @MutSelf public T remove(uint index);
    @MutSelf @RustClosureRefs("0") public void retain<F>((T) -> bool f);
    @MutSelf @RustClosureRefs("0") public void retain_mut<F>((T) -> bool f);
    @MutSelf @RustClosureRefs("0") public void dedup_by_key<F, K>((T) -> K key);
    @MutSelf @RustClosureRefs("0") public void dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public T? pop();
    @MutSelf @RustClosureRefs("0") public T? pop_if((T) -> bool predicate);
    @MutSelf public void append(&mut Vec other);
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public void clear();
    public uint len();
    public bool is_empty();
    @MutSelf @RustBounds("A: Clone") public Vec split_off(uint at);
    @MutSelf public void resize_with<F>(uint new_len, () -> T f);
    @RustRefOut public T[] leak();
    @MutSelf @RustRefOut public MaybeUninit<T>[] spare_capacity_mut();
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_slice(T[] other);
    @MutSelf public void extend_from_within<R>(R src);
    public Vec<T> into_flattened();
    @MutSelf public void dedup();
    @MutSelf @RustBorrowsSelf public Splice<I.IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
    @MutSelf @RustBounds("T: Ord") public void sort();
    @MutSelf @RustClosureRefs("0") public void sort_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_by_key<K, F>((T) -> K f);
    @MutSelf @RustClosureRefs("0") public void sort_by_cached_key<K, F>((T) -> K f);
    @RustBounds("T: Clone") public Vec<T> to_vec();
    @RustBounds("T: Copy") public Vec<T> repeat(uint n);
    public Self.Output concat<Item>();
    public Self.Output join<Separator>(Separator sep);
    public Self.Output connect<Separator>(Separator sep);
    public Vec<ubyte> to_ascii_uppercase();
    public Vec<ubyte> to_ascii_lowercase();
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
    @RustBorrowsSelf @RustClosureRefs("0") public ChunkBy<T, F> chunk_by<F>((T, T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public ChunkByMut<T, F> chunk_by_mut<F>((T, T) -> bool pred);
    public (T[], T[]) split_at(uint mid);
    @MutSelf public (T[], T[]) split_at_mut(uint mid);
    public unsafe (T[], T[]) split_at_unchecked(uint mid);
    @MutSelf public unsafe (T[], T[]) split_at_mut_unchecked(uint mid);
    public (T[], T[])? split_at_checked(uint mid);
    @MutSelf public (T[], T[])? split_at_mut_checked(uint mid);
    @RustBorrowsSelf @RustClosureRefs("0") public Split<T, F> split<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitMut<T, F> split_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusive<T, F> split_inclusive<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public SplitInclusiveMut<T, F> split_inclusive_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("0") public RSplit<T, F> rsplit<F>((T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("0") public RSplitMut<T, F> rsplit_mut<F>((T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public SplitN<T, F> splitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public SplitNMut<T, F> splitn_mut<F>(uint n, (T) -> bool pred);
    @RustBorrowsSelf @RustClosureRefs("1") public RSplitN<T, F> rsplitn<F>(uint n, (T) -> bool pred);
    @MutSelf @RustBorrowsSelf @RustClosureRefs("1") public RSplitNMut<T, F> rsplitn_mut<F>(uint n, (T) -> bool pred);
    @RustBounds("T: PartialEq") public bool contains(&T x);
    @RustBounds("T: PartialEq") public bool starts_with(T[] needle);
    @RustBounds("T: PartialEq") public bool ends_with(T[] needle);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_prefix<P>(&P prefix);
    @RustRefOut @RustBounds("T: PartialEq") public T[]? strip_suffix<P>(&P suffix);
    @RustBounds("T: Ord") public uint binary_search(&T x) throws Error;
    @RustClosureRefs("0") public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    @RustClosureRefs("1") public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @MutSelf @RustBounds("T: Ord") public void sort_unstable();
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by<F>((T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("0") public void sort_unstable_by_key<K, F>((T) -> K f);
    @MutSelf @RustBounds("T: Ord") public (T[], T, T[]) select_nth_unstable(uint index);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by<F>(uint index, (T, T) -> Ordering compare);
    @MutSelf @RustClosureRefs("1") public (T[], T, T[]) select_nth_unstable_by_key<K, F>(uint index, (T) -> K f);
    @MutSelf public void rotate_left(uint mid);
    @MutSelf public void rotate_right(uint k);
    @MutSelf @RustBounds("T: Clone") public void fill(T value);
    @MutSelf public void fill_with<F>(() -> T f);
    @MutSelf @RustBounds("T: Clone") public void clone_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_from_slice(T[] src);
    @MutSelf @RustBounds("T: Copy") public void copy_within<R>(R src, uint dest);
    @MutSelf public void swap_with_slice(&mut T[] other);
    public unsafe (T[], U[], T[]) align_to<U>();
    @MutSelf public unsafe (T[], U[], T[]) align_to_mut<U>();
    @RustBounds("T: PartialOrd") public bool is_sorted();
    @RustClosureRefs("0") public bool is_sorted_by<F>((T, T) -> bool compare);
    @RustClosureRefs("0") public bool is_sorted_by_key<F, K>((T) -> K f);
    @RustClosureRefs("0") public uint partition_point<P>((T) -> bool pred);
    @MutSelf @RustRefOut public Self? split_off_mut<R>(R range);
    @MutSelf @RustRefOut public T? split_off_first();
    @MutSelf @RustRefOut public T? split_off_first_mut();
    @MutSelf @RustRefOut public T? split_off_last();
    @MutSelf @RustRefOut public T? split_off_last_mut();
    @MutSelf public unsafe I.Output[] get_disjoint_unchecked_mut<I>(I[] indices);
    @MutSelf public I.Output[] get_disjoint_mut<I>(I[] indices) throws GetDisjointMutError;
    public uint? element_offset(&T element);
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
    @MutSelf public (Self, MaybeUninit<U>[], Self) align_to_uninit_mut<U>();
    @RustRefOut public String as_str();
    @RustRefOut public ubyte[] as_bytes();
    @MutSelf @RustRefOut @RustBounds("T: Copy") public T[] write_copy_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_clone_of_slice(T[] src);
    @MutSelf @RustRefOut @RustBounds("T: Clone") public T[] write_filled(T value);
    @MutSelf @RustRefOut public T[] write_with<F>((uint) -> T f);
    @MutSelf public (T[], MaybeUninit<T>[]) write_iter<I>(I it);
    @MutSelf @RustRefOut public MaybeUninit<ubyte>[] as_bytes_mut();
    @MutSelf public unsafe void assume_init_drop();
    @RustRefOut public unsafe T[] assume_init_ref();
    @MutSelf @RustRefOut public unsafe T[] assume_init_mut();
    @RustRefOut public T[] as_flattened();
    @MutSelf @RustRefOut public T[] as_flattened_mut();
    @RustBorrowsSelf public Utf8Chunks utf8_chunks();
}

/** A double-ended queue implemented with a growable ring buffer. */
@rust("std::collections::VecDeque")
@RustClone
@RustCollection
public class VecDeque<T, A> implements ToOwned {
    public VecDeque();
    public static VecDeque<T> with_capacity(uint capacity);
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
    @RustBorrowsSelf public Iter<T> iter();
    @MutSelf @RustBorrowsSelf public IterMut<T> iter_mut();
    public (T[], T[]) as_slices();
    @MutSelf public (T[], T[]) as_mut_slices();
    public uint len();
    public bool is_empty();
    @RustBorrowsSelf public Iter<T> range<R>(R range);
    @MutSelf @RustBorrowsSelf public IterMut<T> range_mut<R>(R range);
    @MutSelf @RustBorrowsSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public void clear();
    @RustBounds("T: PartialEq") public bool contains(&T x);
    @RustRefOut public T? front();
    @MutSelf @RustRefOut public T? front_mut();
    @RustRefOut public T? back();
    @MutSelf @RustRefOut public T? back_mut();
    @MutSelf public T? pop_front();
    @MutSelf public T? pop_back();
    @MutSelf @RustClosureRefs("0") public T? pop_front_if((T) -> bool predicate);
    @MutSelf @RustClosureRefs("0") public T? pop_back_if((T) -> bool predicate);
    @MutSelf public void push_front(T value);
    @MutSelf @RustRefOut public T push_front_mut(T value);
    @MutSelf public void push_back(T value);
    @MutSelf @RustRefOut public T push_back_mut(T value);
    @MutSelf public T? swap_remove_front(uint index);
    @MutSelf public T? swap_remove_back(uint index);
    @MutSelf public void insert(uint index, T value);
    @MutSelf @RustRefOut public T insert_mut(uint index, T value);
    @MutSelf public T? remove(uint index);
    @MutSelf @RustBounds("A: Clone") public VecDeque split_off(uint at);
    @MutSelf public void append(&mut VecDeque other);
    @MutSelf @RustClosureRefs("0") public void retain<F>((T) -> bool f);
    @MutSelf @RustClosureRefs("0") public void retain_mut<F>((T) -> bool f);
    @MutSelf public void resize_with(uint new_len, () -> T generator);
    @MutSelf @RustRefOut public T[] make_contiguous();
    @MutSelf public void rotate_left(uint n);
    @MutSelf public void rotate_right(uint n);
    @RustBounds("T: Ord") public uint binary_search(&T x) throws Error;
    @RustClosureRefs("0") public uint binary_search_by<F>((T) -> Ordering f) throws Error;
    @RustClosureRefs("1") public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws Error;
    @RustClosureRefs("0") public uint partition_point<P>((T) -> bool pred);
    @MutSelf public void resize(uint new_len, T value);
}

/** A type indicating whether a timed wait on a condition variable returned */
@rust("std::sync::WaitTimeoutResult")
@RustClone
public class WaitTimeoutResult implements ToOwned {
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
public class Waker {
    public Waker(Object* data, &RawWakerVTable vtable);
    public void wake();
    public void wake_by_ref();
    public bool will_wake(&Waker other);
    public static unsafe Waker from_raw(RawWaker waker);
    @RustRefOut public static Waker noop();
    public Object* data();
    @RustRefOut public RawWakerVTable vtable();
}

/** `Weak` is a version of [`Rc`] that holds a non-owning reference to the */
@rust("std::rc::Weak")
@RustClone
public class Weak<T, A> implements ToOwned {
    public Weak();
    public static unsafe Weak from_raw(T* ptr);
    public T* into_raw();
    public T* as_ptr();
    @RustBounds("A: Clone") public T? upgrade();
    public uint strong_count();
    public uint weak_count();
    public bool ptr_eq(&Weak other);
}

/** An iterator over overlapping subslices of length `size`. */
@rust("std::slice::Windows")
@RustClone
public class Windows<T> implements RustIterator {
    @MutSelf @RustRefOut public T[]? next();
}

/** Provides intentionally-wrapped arithmetic on `T`. */
@rust("std::num::Wrapping")
@RustClone
public class Wrapping<T> {
    @RustDefault public Wrapping();
    public Wrapping reverse_bits();
    public static Wrapping from_be(Wrapping x);
    public static Wrapping from_le(Wrapping x);
    public Wrapping<int> abs();
    public Wrapping<int> signum();
    public bool is_positive();
    public bool is_negative();
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
public class WriterPanicked implements ToString {
    public Vec<ubyte> into_inner();
}

/** An iterator that produces directory paths from XDG environment configuration. */
@rust("std::os::unix::xdg::XdgDirsIter")
@RustClone
public class XdgDirsIter implements RustIterator, ToOwned {
    @MutSelf public PathBuf? next();
}

/** An iterator that iterates two other iterators simultaneously. */
@rust("std::iter::Zip")
@RustClone
public class Zip<A, B> implements RustIterator {
    @MutSelf public (A.Item, B.Item)? next();
}

@rust("std::process::abort")
public never abort();

@rust("std::process::abort_immediate")
public never abort_immediate();

@rust("std::path::absolute")
public PathBuf absolute<P>(P path) throws Error;

@rust("std::mem::align_of")
public uint align_of<T>();

@rust("std::mem::align_of_val")
public uint align_of_val<T>(&T val);

@rust("std::alloc::alloc")
public unsafe ubyte* alloc(Layout layout);

@rust("std::alloc::alloc_zeroed")
public unsafe ubyte* alloc_zeroed(Layout layout);

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


@rust("bool")
@RustPrimitive("bool")
public class bool_methods {
    public T? then_some<T>(T t);
    public T? then<T, F>(() -> T f);
}

/** Equivalent to C's `void` type when used as a [pointer]. */
@rust("std::ffi::c_void")
public enum c_void {
    // (variants not represented)
}

@rust("std::os::unix::xdg::cache_home_dir")
public PathBuf cache_home_dir();

@rust("std::fs::canonicalize")
public PathBuf canonicalize<P>(P path) throws Error;

@rust("std::panic::catch_unwind")
public R catch_unwind<F, R>(() -> R f) throws Error;

@rust("char")
@RustPrimitive("char")
public class char_methods {
    public static DecodeUtf16<I.IntoIter> decode_utf16<I>(I iter);
    public static char? from_u32(u32 i);
    public static unsafe char from_u32_unchecked(u32 i);
    public static char? from_digit(u32 num, u32 radix);
    public bool is_digit(u32 radix);
    public u32? to_digit(u32 radix);
    public EscapeUnicode escape_unicode();
    public EscapeDebug escape_debug();
    public EscapeDefault escape_default();
    public uint len_utf8();
    public uint len_utf16();
    @RustRefOut public String encode_utf8(&mut ubyte[] dst);
    @RustRefOut public ushort[] encode_utf16(&mut ushort[] dst);
    public bool is_alphabetic();
    public bool is_lowercase();
    public bool is_uppercase();
    public bool is_numeric();
    public bool is_alphanumeric();
    public bool is_whitespace();
    public bool is_control();
    public ToLowercase to_lowercase();
    public ToUppercase to_uppercase();
    public ToCasefold to_casefold_unnormalized();
    public bool is_ascii();
    public char to_ascii_uppercase();
    public char to_ascii_lowercase();
    public bool eq_ignore_ascii_case(&char other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    public bool is_ascii_alphabetic();
    public bool is_ascii_uppercase();
    public bool is_ascii_lowercase();
    public bool is_ascii_alphanumeric();
    public bool is_ascii_digit();
    public bool is_ascii_hexdigit();
    public bool is_ascii_punctuation();
    public bool is_ascii_graphic();
    public bool is_ascii_whitespace();
    public bool is_ascii_control();
}

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

@rust("f32")
@RustPrimitive("f32")
public class f32_methods {
    public float floor();
    public float ceil();
    public float round();
    public float round_ties_even();
    public float trunc();
    public float fract();
    public float mul_add(float a, float b);
    public float div_euclid(float rhs);
    public float rem_euclid(float rhs);
    public float powi(i32 n);
    public float powf(float n);
    public float sqrt();
    public float exp();
    public float exp2();
    public float ln();
    public float log(float base);
    public float log2();
    public float log10();
    public float abs_sub(float other);
    public float cbrt();
    public float hypot(float other);
    public float sin();
    public float cos();
    public float tan();
    public float asin();
    public float acos();
    public float atan();
    public float atan2(float other);
    public (float, float) sin_cos();
    public float exp_m1();
    public float ln_1p();
    public float sinh();
    public float cosh();
    public float tanh();
    public float asinh();
    public float acosh();
    public float atanh();
    public bool is_nan();
    public bool is_infinite();
    public bool is_finite();
    public bool is_subnormal();
    public bool is_normal();
    public FpCategory classify();
    public bool is_sign_positive();
    public bool is_sign_negative();
    public float next_up();
    public float next_down();
    public float recip();
    public float to_degrees();
    public float to_radians();
    public float max(float other);
    public float min(float other);
    public float midpoint(float other);
    public unsafe Int to_int_unchecked<Int>();
    public u32 to_bits();
    public static float from_bits(u32 v);
    public ubyte[4] to_be_bytes();
    public ubyte[4] to_le_bytes();
    public ubyte[4] to_ne_bytes();
    public static float from_be_bytes(ubyte[4] bytes);
    public static float from_le_bytes(ubyte[4] bytes);
    public static float from_ne_bytes(ubyte[4] bytes);
    public Ordering total_cmp(&float other);
    public float clamp(float min, float max);
    public float abs();
    public float signum();
    public float copysign(float sign);
}

@rust("f64")
@RustPrimitive("f64")
public class f64_methods {
    public double floor();
    public double ceil();
    public double round();
    public double round_ties_even();
    public double trunc();
    public double fract();
    public double mul_add(double a, double b);
    public double div_euclid(double rhs);
    public double rem_euclid(double rhs);
    public double powi(i32 n);
    public double powf(double n);
    public double sqrt();
    public double exp();
    public double exp2();
    public double ln();
    public double log(double base);
    public double log2();
    public double log10();
    public double abs_sub(double other);
    public double cbrt();
    public double hypot(double other);
    public double sin();
    public double cos();
    public double tan();
    public double asin();
    public double acos();
    public double atan();
    public double atan2(double other);
    public (double, double) sin_cos();
    public double exp_m1();
    public double ln_1p();
    public double sinh();
    public double cosh();
    public double tanh();
    public double asinh();
    public double acosh();
    public double atanh();
    public bool is_nan();
    public bool is_infinite();
    public bool is_finite();
    public bool is_subnormal();
    public bool is_normal();
    public FpCategory classify();
    public bool is_sign_positive();
    public bool is_sign_negative();
    public double next_up();
    public double next_down();
    public double recip();
    public double to_degrees();
    public double to_radians();
    public double max(double other);
    public double min(double other);
    public double midpoint(double other);
    public unsafe Int to_int_unchecked<Int>();
    public ulong to_bits();
    public static double from_bits(ulong v);
    public ubyte[8] to_be_bytes();
    public ubyte[8] to_le_bytes();
    public ubyte[8] to_ne_bytes();
    public static double from_be_bytes(ubyte[8] bytes);
    public static double from_le_bytes(ubyte[8] bytes);
    public static double from_ne_bytes(ubyte[8] bytes);
    public Ordering total_cmp(&double other);
    public double clamp(double min, double max);
    public double abs();
    public double signum();
    public double copysign(double sign);
}

@rust("std::os::unix::fs::fchown")
public void fchown<F>(F fd, u32? uid, u32? gid) throws Error;

@rust("std::fmt::format")
public String format(Arguments args);

@rust("std::str::from_boxed_utf8_unchecked")
public unsafe String from_boxed_utf8_unchecked(ubyte[] v);

public type gid_t = u32;


@rust("std::alloc::handle_alloc_error")
public never handle_alloc_error(Layout layout);

@rust("std::fs::hard_link")
public void hard_link<P, Q>(P original, Q link) throws Error;

@rust("std::env::home_dir")
public PathBuf? home_dir();

@rust("i16")
@RustPrimitive("i16")
public class i16_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public ushort cast_unsigned();
    public ushort saturating_cast_unsigned();
    public ushort? checked_cast_unsigned();
    public ushort strict_cast_unsigned();
    public short rotate_left(u32 n);
    public short rotate_right(u32 n);
    public short swap_bytes();
    public short reverse_bits();
    public static short from_be(short x);
    public static short from_le(short x);
    public short to_be();
    public short to_le();
    public short? checked_add(short rhs);
    public short strict_add(short rhs);
    public unsafe short unchecked_add(short rhs);
    public short? checked_add_unsigned(ushort rhs);
    public short strict_add_unsigned(ushort rhs);
    public short? checked_sub(short rhs);
    public short strict_sub(short rhs);
    public unsafe short unchecked_sub(short rhs);
    public short? checked_sub_unsigned(ushort rhs);
    public short strict_sub_unsigned(ushort rhs);
    public short? checked_mul(short rhs);
    public short strict_mul(short rhs);
    public unsafe short unchecked_mul(short rhs);
    public short? checked_div(short rhs);
    public short strict_div(short rhs);
    public short? checked_div_euclid(short rhs);
    public short strict_div_euclid(short rhs);
    public short? checked_rem(short rhs);
    public short strict_rem(short rhs);
    public short? checked_rem_euclid(short rhs);
    public short strict_rem_euclid(short rhs);
    public short? checked_neg();
    public unsafe short unchecked_neg();
    public short strict_neg();
    public short? checked_shl(u32 rhs);
    public short strict_shl(u32 rhs);
    public unsafe short unchecked_shl(u32 rhs);
    public short unbounded_shl(u32 rhs);
    public short? checked_shr(u32 rhs);
    public short strict_shr(u32 rhs);
    public unsafe short unchecked_shr(u32 rhs);
    public short unbounded_shr(u32 rhs);
    public short? checked_abs();
    public short strict_abs();
    public short? checked_pow(u32 exp);
    public short strict_pow(u32 exp);
    public short? checked_isqrt();
    public short saturating_add(short rhs);
    public short saturating_add_unsigned(ushort rhs);
    public short saturating_sub(short rhs);
    public short saturating_sub_unsigned(ushort rhs);
    public short saturating_neg();
    public short saturating_abs();
    public short saturating_mul(short rhs);
    public short saturating_div(short rhs);
    public short saturating_pow(u32 exp);
    public short wrapping_add(short rhs);
    public short wrapping_add_unsigned(ushort rhs);
    public short wrapping_sub(short rhs);
    public short wrapping_sub_unsigned(ushort rhs);
    public short wrapping_mul(short rhs);
    public short wrapping_div(short rhs);
    public short wrapping_div_euclid(short rhs);
    public short wrapping_rem(short rhs);
    public short wrapping_rem_euclid(short rhs);
    public short wrapping_neg();
    public short wrapping_shl(u32 rhs);
    public short wrapping_shr(u32 rhs);
    public short wrapping_abs();
    public ushort unsigned_abs();
    public short wrapping_pow(u32 exp);
    public (short, bool) overflowing_add(short rhs);
    public (short, bool) overflowing_add_unsigned(ushort rhs);
    public (short, bool) overflowing_sub(short rhs);
    public (short, bool) overflowing_sub_unsigned(ushort rhs);
    public (short, bool) overflowing_mul(short rhs);
    public (short, bool) overflowing_div(short rhs);
    public (short, bool) overflowing_div_euclid(short rhs);
    public (short, bool) overflowing_rem(short rhs);
    public (short, bool) overflowing_rem_euclid(short rhs);
    public (short, bool) overflowing_neg();
    public (short, bool) overflowing_shl(u32 rhs);
    public (short, bool) overflowing_shr(u32 rhs);
    public (short, bool) overflowing_abs();
    public (short, bool) overflowing_pow(u32 exp);
    public short pow(u32 exp);
    public short isqrt();
    public short div_euclid(short rhs);
    public short rem_euclid(short rhs);
    public u32 ilog(short base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(short base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public short abs();
    public ushort abs_diff(short other);
    public short signum();
    public bool is_positive();
    public bool is_negative();
    public ubyte[2] to_be_bytes();
    public ubyte[2] to_le_bytes();
    public ubyte[2] to_ne_bytes();
    public static short from_be_bytes(ubyte[2] bytes);
    public static short from_le_bytes(ubyte[2] bytes);
    public static short from_ne_bytes(ubyte[2] bytes);
    public static short min_value();
    public static short max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public short midpoint(short rhs);
    public static short from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("i32")
@RustPrimitive("i32")
public class i32_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public u32 cast_unsigned();
    public u32 saturating_cast_unsigned();
    public u32? checked_cast_unsigned();
    public u32 strict_cast_unsigned();
    public i32 rotate_left(u32 n);
    public i32 rotate_right(u32 n);
    public i32 swap_bytes();
    public i32 reverse_bits();
    public static i32 from_be(i32 x);
    public static i32 from_le(i32 x);
    public i32 to_be();
    public i32 to_le();
    public i32? checked_add(i32 rhs);
    public i32 strict_add(i32 rhs);
    public unsafe i32 unchecked_add(i32 rhs);
    public i32? checked_add_unsigned(u32 rhs);
    public i32 strict_add_unsigned(u32 rhs);
    public i32? checked_sub(i32 rhs);
    public i32 strict_sub(i32 rhs);
    public unsafe i32 unchecked_sub(i32 rhs);
    public i32? checked_sub_unsigned(u32 rhs);
    public i32 strict_sub_unsigned(u32 rhs);
    public i32? checked_mul(i32 rhs);
    public i32 strict_mul(i32 rhs);
    public unsafe i32 unchecked_mul(i32 rhs);
    public i32? checked_div(i32 rhs);
    public i32 strict_div(i32 rhs);
    public i32? checked_div_euclid(i32 rhs);
    public i32 strict_div_euclid(i32 rhs);
    public i32? checked_rem(i32 rhs);
    public i32 strict_rem(i32 rhs);
    public i32? checked_rem_euclid(i32 rhs);
    public i32 strict_rem_euclid(i32 rhs);
    public i32? checked_neg();
    public unsafe i32 unchecked_neg();
    public i32 strict_neg();
    public i32? checked_shl(u32 rhs);
    public i32 strict_shl(u32 rhs);
    public unsafe i32 unchecked_shl(u32 rhs);
    public i32 unbounded_shl(u32 rhs);
    public i32? checked_shr(u32 rhs);
    public i32 strict_shr(u32 rhs);
    public unsafe i32 unchecked_shr(u32 rhs);
    public i32 unbounded_shr(u32 rhs);
    public i32? checked_abs();
    public i32 strict_abs();
    public i32? checked_pow(u32 exp);
    public i32 strict_pow(u32 exp);
    public i32? checked_isqrt();
    public i32 saturating_add(i32 rhs);
    public i32 saturating_add_unsigned(u32 rhs);
    public i32 saturating_sub(i32 rhs);
    public i32 saturating_sub_unsigned(u32 rhs);
    public i32 saturating_neg();
    public i32 saturating_abs();
    public i32 saturating_mul(i32 rhs);
    public i32 saturating_div(i32 rhs);
    public i32 saturating_pow(u32 exp);
    public i32 wrapping_add(i32 rhs);
    public i32 wrapping_add_unsigned(u32 rhs);
    public i32 wrapping_sub(i32 rhs);
    public i32 wrapping_sub_unsigned(u32 rhs);
    public i32 wrapping_mul(i32 rhs);
    public i32 wrapping_div(i32 rhs);
    public i32 wrapping_div_euclid(i32 rhs);
    public i32 wrapping_rem(i32 rhs);
    public i32 wrapping_rem_euclid(i32 rhs);
    public i32 wrapping_neg();
    public i32 wrapping_shl(u32 rhs);
    public i32 wrapping_shr(u32 rhs);
    public i32 wrapping_abs();
    public u32 unsigned_abs();
    public i32 wrapping_pow(u32 exp);
    public (i32, bool) overflowing_add(i32 rhs);
    public (i32, bool) overflowing_add_unsigned(u32 rhs);
    public (i32, bool) overflowing_sub(i32 rhs);
    public (i32, bool) overflowing_sub_unsigned(u32 rhs);
    public (i32, bool) overflowing_mul(i32 rhs);
    public (i32, bool) overflowing_div(i32 rhs);
    public (i32, bool) overflowing_div_euclid(i32 rhs);
    public (i32, bool) overflowing_rem(i32 rhs);
    public (i32, bool) overflowing_rem_euclid(i32 rhs);
    public (i32, bool) overflowing_neg();
    public (i32, bool) overflowing_shl(u32 rhs);
    public (i32, bool) overflowing_shr(u32 rhs);
    public (i32, bool) overflowing_abs();
    public (i32, bool) overflowing_pow(u32 exp);
    public i32 pow(u32 exp);
    public i32 isqrt();
    public i32 div_euclid(i32 rhs);
    public i32 rem_euclid(i32 rhs);
    public u32 ilog(i32 base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(i32 base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public i32 abs();
    public u32 abs_diff(i32 other);
    public i32 signum();
    public bool is_positive();
    public bool is_negative();
    public ubyte[4] to_be_bytes();
    public ubyte[4] to_le_bytes();
    public ubyte[4] to_ne_bytes();
    public static i32 from_be_bytes(ubyte[4] bytes);
    public static i32 from_le_bytes(ubyte[4] bytes);
    public static i32 from_ne_bytes(ubyte[4] bytes);
    public static i32 min_value();
    public static i32 max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public i32 midpoint(i32 rhs);
    public static i32 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("i64")
@RustPrimitive("i64")
public class i64_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public ulong cast_unsigned();
    public ulong saturating_cast_unsigned();
    public ulong? checked_cast_unsigned();
    public ulong strict_cast_unsigned();
    public long rotate_left(u32 n);
    public long rotate_right(u32 n);
    public long swap_bytes();
    public long reverse_bits();
    public static long from_be(long x);
    public static long from_le(long x);
    public long to_be();
    public long to_le();
    public long? checked_add(long rhs);
    public long strict_add(long rhs);
    public unsafe long unchecked_add(long rhs);
    public long? checked_add_unsigned(ulong rhs);
    public long strict_add_unsigned(ulong rhs);
    public long? checked_sub(long rhs);
    public long strict_sub(long rhs);
    public unsafe long unchecked_sub(long rhs);
    public long? checked_sub_unsigned(ulong rhs);
    public long strict_sub_unsigned(ulong rhs);
    public long? checked_mul(long rhs);
    public long strict_mul(long rhs);
    public unsafe long unchecked_mul(long rhs);
    public long? checked_div(long rhs);
    public long strict_div(long rhs);
    public long? checked_div_euclid(long rhs);
    public long strict_div_euclid(long rhs);
    public long? checked_rem(long rhs);
    public long strict_rem(long rhs);
    public long? checked_rem_euclid(long rhs);
    public long strict_rem_euclid(long rhs);
    public long? checked_neg();
    public unsafe long unchecked_neg();
    public long strict_neg();
    public long? checked_shl(u32 rhs);
    public long strict_shl(u32 rhs);
    public unsafe long unchecked_shl(u32 rhs);
    public long unbounded_shl(u32 rhs);
    public long? checked_shr(u32 rhs);
    public long strict_shr(u32 rhs);
    public unsafe long unchecked_shr(u32 rhs);
    public long unbounded_shr(u32 rhs);
    public long? checked_abs();
    public long strict_abs();
    public long? checked_pow(u32 exp);
    public long strict_pow(u32 exp);
    public long? checked_isqrt();
    public long saturating_add(long rhs);
    public long saturating_add_unsigned(ulong rhs);
    public long saturating_sub(long rhs);
    public long saturating_sub_unsigned(ulong rhs);
    public long saturating_neg();
    public long saturating_abs();
    public long saturating_mul(long rhs);
    public long saturating_div(long rhs);
    public long saturating_pow(u32 exp);
    public long wrapping_add(long rhs);
    public long wrapping_add_unsigned(ulong rhs);
    public long wrapping_sub(long rhs);
    public long wrapping_sub_unsigned(ulong rhs);
    public long wrapping_mul(long rhs);
    public long wrapping_div(long rhs);
    public long wrapping_div_euclid(long rhs);
    public long wrapping_rem(long rhs);
    public long wrapping_rem_euclid(long rhs);
    public long wrapping_neg();
    public long wrapping_shl(u32 rhs);
    public long wrapping_shr(u32 rhs);
    public long wrapping_abs();
    public ulong unsigned_abs();
    public long wrapping_pow(u32 exp);
    public (long, bool) overflowing_add(long rhs);
    public (long, bool) overflowing_add_unsigned(ulong rhs);
    public (long, bool) overflowing_sub(long rhs);
    public (long, bool) overflowing_sub_unsigned(ulong rhs);
    public (long, bool) overflowing_mul(long rhs);
    public (long, bool) overflowing_div(long rhs);
    public (long, bool) overflowing_div_euclid(long rhs);
    public (long, bool) overflowing_rem(long rhs);
    public (long, bool) overflowing_rem_euclid(long rhs);
    public (long, bool) overflowing_neg();
    public (long, bool) overflowing_shl(u32 rhs);
    public (long, bool) overflowing_shr(u32 rhs);
    public (long, bool) overflowing_abs();
    public (long, bool) overflowing_pow(u32 exp);
    public long pow(u32 exp);
    public long isqrt();
    public long div_euclid(long rhs);
    public long rem_euclid(long rhs);
    public u32 ilog(long base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(long base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public long abs();
    public ulong abs_diff(long other);
    public long signum();
    public bool is_positive();
    public bool is_negative();
    public ubyte[8] to_be_bytes();
    public ubyte[8] to_le_bytes();
    public ubyte[8] to_ne_bytes();
    public static long from_be_bytes(ubyte[8] bytes);
    public static long from_le_bytes(ubyte[8] bytes);
    public static long from_ne_bytes(ubyte[8] bytes);
    public static long min_value();
    public static long max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public long midpoint(long rhs);
    public static long from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("i8")
@RustPrimitive("i8")
public class i8_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public ubyte cast_unsigned();
    public ubyte saturating_cast_unsigned();
    public ubyte? checked_cast_unsigned();
    public ubyte strict_cast_unsigned();
    public byte rotate_left(u32 n);
    public byte rotate_right(u32 n);
    public byte swap_bytes();
    public byte reverse_bits();
    public static byte from_be(byte x);
    public static byte from_le(byte x);
    public byte to_be();
    public byte to_le();
    public byte? checked_add(byte rhs);
    public byte strict_add(byte rhs);
    public unsafe byte unchecked_add(byte rhs);
    public byte? checked_add_unsigned(ubyte rhs);
    public byte strict_add_unsigned(ubyte rhs);
    public byte? checked_sub(byte rhs);
    public byte strict_sub(byte rhs);
    public unsafe byte unchecked_sub(byte rhs);
    public byte? checked_sub_unsigned(ubyte rhs);
    public byte strict_sub_unsigned(ubyte rhs);
    public byte? checked_mul(byte rhs);
    public byte strict_mul(byte rhs);
    public unsafe byte unchecked_mul(byte rhs);
    public byte? checked_div(byte rhs);
    public byte strict_div(byte rhs);
    public byte? checked_div_euclid(byte rhs);
    public byte strict_div_euclid(byte rhs);
    public byte? checked_rem(byte rhs);
    public byte strict_rem(byte rhs);
    public byte? checked_rem_euclid(byte rhs);
    public byte strict_rem_euclid(byte rhs);
    public byte? checked_neg();
    public unsafe byte unchecked_neg();
    public byte strict_neg();
    public byte? checked_shl(u32 rhs);
    public byte strict_shl(u32 rhs);
    public unsafe byte unchecked_shl(u32 rhs);
    public byte unbounded_shl(u32 rhs);
    public byte? checked_shr(u32 rhs);
    public byte strict_shr(u32 rhs);
    public unsafe byte unchecked_shr(u32 rhs);
    public byte unbounded_shr(u32 rhs);
    public byte? checked_abs();
    public byte strict_abs();
    public byte? checked_pow(u32 exp);
    public byte strict_pow(u32 exp);
    public byte? checked_isqrt();
    public byte saturating_add(byte rhs);
    public byte saturating_add_unsigned(ubyte rhs);
    public byte saturating_sub(byte rhs);
    public byte saturating_sub_unsigned(ubyte rhs);
    public byte saturating_neg();
    public byte saturating_abs();
    public byte saturating_mul(byte rhs);
    public byte saturating_div(byte rhs);
    public byte saturating_pow(u32 exp);
    public byte wrapping_add(byte rhs);
    public byte wrapping_add_unsigned(ubyte rhs);
    public byte wrapping_sub(byte rhs);
    public byte wrapping_sub_unsigned(ubyte rhs);
    public byte wrapping_mul(byte rhs);
    public byte wrapping_div(byte rhs);
    public byte wrapping_div_euclid(byte rhs);
    public byte wrapping_rem(byte rhs);
    public byte wrapping_rem_euclid(byte rhs);
    public byte wrapping_neg();
    public byte wrapping_shl(u32 rhs);
    public byte wrapping_shr(u32 rhs);
    public byte wrapping_abs();
    public ubyte unsigned_abs();
    public byte wrapping_pow(u32 exp);
    public (byte, bool) overflowing_add(byte rhs);
    public (byte, bool) overflowing_add_unsigned(ubyte rhs);
    public (byte, bool) overflowing_sub(byte rhs);
    public (byte, bool) overflowing_sub_unsigned(ubyte rhs);
    public (byte, bool) overflowing_mul(byte rhs);
    public (byte, bool) overflowing_div(byte rhs);
    public (byte, bool) overflowing_div_euclid(byte rhs);
    public (byte, bool) overflowing_rem(byte rhs);
    public (byte, bool) overflowing_rem_euclid(byte rhs);
    public (byte, bool) overflowing_neg();
    public (byte, bool) overflowing_shl(u32 rhs);
    public (byte, bool) overflowing_shr(u32 rhs);
    public (byte, bool) overflowing_abs();
    public (byte, bool) overflowing_pow(u32 exp);
    public byte pow(u32 exp);
    public byte isqrt();
    public byte div_euclid(byte rhs);
    public byte rem_euclid(byte rhs);
    public u32 ilog(byte base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(byte base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public byte abs();
    public ubyte abs_diff(byte other);
    public byte signum();
    public bool is_positive();
    public bool is_negative();
    public ubyte[1] to_be_bytes();
    public ubyte[1] to_le_bytes();
    public ubyte[1] to_ne_bytes();
    public static byte from_be_bytes(ubyte[1] bytes);
    public static byte from_le_bytes(ubyte[1] bytes);
    public static byte from_ne_bytes(ubyte[1] bytes);
    public static byte min_value();
    public static byte max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public byte midpoint(byte rhs);
    public static byte from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("std::process::id")
public u32 id();

public type ino_t = ulong;


@rust("std::path::is_separator")
public bool is_separator(char c);

@rust("isize")
@RustPrimitive("isize")
public class isize_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public uint cast_unsigned();
    public uint saturating_cast_unsigned();
    public uint? checked_cast_unsigned();
    public uint strict_cast_unsigned();
    public int rotate_left(u32 n);
    public int rotate_right(u32 n);
    public int swap_bytes();
    public int reverse_bits();
    public static int from_be(int x);
    public static int from_le(int x);
    public int to_be();
    public int to_le();
    public int? checked_add(int rhs);
    public int strict_add(int rhs);
    public unsafe int unchecked_add(int rhs);
    public int? checked_add_unsigned(uint rhs);
    public int strict_add_unsigned(uint rhs);
    public int? checked_sub(int rhs);
    public int strict_sub(int rhs);
    public unsafe int unchecked_sub(int rhs);
    public int? checked_sub_unsigned(uint rhs);
    public int strict_sub_unsigned(uint rhs);
    public int? checked_mul(int rhs);
    public int strict_mul(int rhs);
    public unsafe int unchecked_mul(int rhs);
    public int? checked_div(int rhs);
    public int strict_div(int rhs);
    public int? checked_div_euclid(int rhs);
    public int strict_div_euclid(int rhs);
    public int? checked_rem(int rhs);
    public int strict_rem(int rhs);
    public int? checked_rem_euclid(int rhs);
    public int strict_rem_euclid(int rhs);
    public int? checked_neg();
    public unsafe int unchecked_neg();
    public int strict_neg();
    public int? checked_shl(u32 rhs);
    public int strict_shl(u32 rhs);
    public unsafe int unchecked_shl(u32 rhs);
    public int unbounded_shl(u32 rhs);
    public int? checked_shr(u32 rhs);
    public int strict_shr(u32 rhs);
    public unsafe int unchecked_shr(u32 rhs);
    public int unbounded_shr(u32 rhs);
    public int? checked_abs();
    public int strict_abs();
    public int? checked_pow(u32 exp);
    public int strict_pow(u32 exp);
    public int? checked_isqrt();
    public int saturating_add(int rhs);
    public int saturating_add_unsigned(uint rhs);
    public int saturating_sub(int rhs);
    public int saturating_sub_unsigned(uint rhs);
    public int saturating_neg();
    public int saturating_abs();
    public int saturating_mul(int rhs);
    public int saturating_div(int rhs);
    public int saturating_pow(u32 exp);
    public int wrapping_add(int rhs);
    public int wrapping_add_unsigned(uint rhs);
    public int wrapping_sub(int rhs);
    public int wrapping_sub_unsigned(uint rhs);
    public int wrapping_mul(int rhs);
    public int wrapping_div(int rhs);
    public int wrapping_div_euclid(int rhs);
    public int wrapping_rem(int rhs);
    public int wrapping_rem_euclid(int rhs);
    public int wrapping_neg();
    public int wrapping_shl(u32 rhs);
    public int wrapping_shr(u32 rhs);
    public int wrapping_abs();
    public uint unsigned_abs();
    public int wrapping_pow(u32 exp);
    public (int, bool) overflowing_add(int rhs);
    public (int, bool) overflowing_add_unsigned(uint rhs);
    public (int, bool) overflowing_sub(int rhs);
    public (int, bool) overflowing_sub_unsigned(uint rhs);
    public (int, bool) overflowing_mul(int rhs);
    public (int, bool) overflowing_div(int rhs);
    public (int, bool) overflowing_div_euclid(int rhs);
    public (int, bool) overflowing_rem(int rhs);
    public (int, bool) overflowing_rem_euclid(int rhs);
    public (int, bool) overflowing_neg();
    public (int, bool) overflowing_shl(u32 rhs);
    public (int, bool) overflowing_shr(u32 rhs);
    public (int, bool) overflowing_abs();
    public (int, bool) overflowing_pow(u32 exp);
    public int pow(u32 exp);
    public int isqrt();
    public int div_euclid(int rhs);
    public int rem_euclid(int rhs);
    public u32 ilog(int base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(int base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public int abs();
    public uint abs_diff(int other);
    public int signum();
    public bool is_positive();
    public bool is_negative();
    public ubyte[8] to_be_bytes();
    public ubyte[8] to_le_bytes();
    public ubyte[8] to_ne_bytes();
    public static int from_be_bytes(ubyte[8] bytes);
    public static int from_le_bytes(ubyte[8] bytes);
    public static int from_ne_bytes(ubyte[8] bytes);
    public static int min_value();
    public static int max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public int midpoint(int rhs);
    public static int from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("std::env::join_paths")
public OsString join_paths<I, T>(I paths) throws JoinPathsError;

@rust("std::os::unix::fs::lchown")
public void lchown<P>(P dir, u32? uid, u32? gid) throws Error;

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

@rust("std::panic::resume_unwind")
public never resume_unwind(Any payload);

@rust("std::thread::scope")
@RustClosureRefs("0") public T scope<F, T>((Scope) -> T f);

@rust("std::env::set_current_dir")
public void set_current_dir<P>(P path) throws Error;

@rust("std::panic::set_hook")
public void set_hook(Fn hook);

@rust("std::fs::set_permissions")
public void set_permissions<P>(P path, Permissions perm) throws Error;

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

@rust("std::panic::take_hook")
public Fn take_hook();

@rust("std::env::temp_dir")
public PathBuf temp_dir();

public type time_t = long;


@rust("u16")
@RustPrimitive("u16")
public class u16_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public short cast_signed();
    public short saturating_cast_signed();
    public short? checked_cast_signed();
    public short strict_cast_signed();
    public ushort rotate_left(u32 n);
    public ushort rotate_right(u32 n);
    public ushort swap_bytes();
    public ushort reverse_bits();
    public static ushort from_be(ushort x);
    public static ushort from_le(ushort x);
    public ushort to_be();
    public ushort to_le();
    public ushort? checked_add(ushort rhs);
    public ushort strict_add(ushort rhs);
    public unsafe ushort unchecked_add(ushort rhs);
    public ushort? checked_add_signed(short rhs);
    public ushort strict_add_signed(short rhs);
    public ushort? checked_sub(ushort rhs);
    public ushort strict_sub(ushort rhs);
    public unsafe ushort unchecked_sub(ushort rhs);
    public ushort? checked_sub_signed(short rhs);
    public ushort strict_sub_signed(short rhs);
    public short? checked_signed_diff(ushort rhs);
    public ushort? checked_mul(ushort rhs);
    public ushort strict_mul(ushort rhs);
    public unsafe ushort unchecked_mul(ushort rhs);
    public ushort? checked_div(ushort rhs);
    public ushort strict_div(ushort rhs);
    public ushort? checked_div_euclid(ushort rhs);
    public ushort strict_div_euclid(ushort rhs);
    public ushort? checked_rem(ushort rhs);
    public ushort strict_rem(ushort rhs);
    public ushort? checked_rem_euclid(ushort rhs);
    public ushort strict_rem_euclid(ushort rhs);
    public u32 ilog(ushort base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(ushort base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public ushort? checked_neg();
    public ushort strict_neg();
    public ushort? checked_shl(u32 rhs);
    public ushort strict_shl(u32 rhs);
    public unsafe ushort unchecked_shl(u32 rhs);
    public ushort unbounded_shl(u32 rhs);
    public ushort? checked_shr(u32 rhs);
    public ushort strict_shr(u32 rhs);
    public unsafe ushort unchecked_shr(u32 rhs);
    public ushort unbounded_shr(u32 rhs);
    public ushort? checked_pow(u32 exp);
    public ushort strict_pow(u32 exp);
    public ushort saturating_add(ushort rhs);
    public ushort saturating_add_signed(short rhs);
    public ushort saturating_sub(ushort rhs);
    public ushort saturating_sub_signed(short rhs);
    public ushort saturating_mul(ushort rhs);
    public ushort saturating_div(ushort rhs);
    public ushort saturating_pow(u32 exp);
    public ushort wrapping_add(ushort rhs);
    public ushort wrapping_add_signed(short rhs);
    public ushort wrapping_sub(ushort rhs);
    public ushort wrapping_sub_signed(short rhs);
    public ushort wrapping_mul(ushort rhs);
    public ushort wrapping_div(ushort rhs);
    public ushort wrapping_div_euclid(ushort rhs);
    public ushort wrapping_rem(ushort rhs);
    public ushort wrapping_rem_euclid(ushort rhs);
    public ushort wrapping_neg();
    public ushort wrapping_shl(u32 rhs);
    public ushort wrapping_shr(u32 rhs);
    public ushort wrapping_pow(u32 exp);
    public (ushort, bool) overflowing_add(ushort rhs);
    public (ushort, bool) carrying_add(ushort rhs, bool carry);
    public (ushort, bool) overflowing_add_signed(short rhs);
    public (ushort, bool) overflowing_sub(ushort rhs);
    public (ushort, bool) borrowing_sub(ushort rhs, bool borrow);
    public (ushort, bool) overflowing_sub_signed(short rhs);
    public ushort abs_diff(ushort other);
    public (ushort, bool) overflowing_mul(ushort rhs);
    public (ushort, ushort) carrying_mul(ushort rhs, ushort carry);
    public (ushort, ushort) carrying_mul_add(ushort rhs, ushort carry, ushort add);
    public (ushort, bool) overflowing_div(ushort rhs);
    public (ushort, bool) overflowing_div_euclid(ushort rhs);
    public (ushort, bool) overflowing_rem(ushort rhs);
    public (ushort, bool) overflowing_rem_euclid(ushort rhs);
    public (ushort, bool) overflowing_neg();
    public (ushort, bool) overflowing_shl(u32 rhs);
    public (ushort, bool) overflowing_shr(u32 rhs);
    public (ushort, bool) overflowing_pow(u32 exp);
    public ushort pow(u32 exp);
    public ushort isqrt();
    public ushort div_euclid(ushort rhs);
    public ushort rem_euclid(ushort rhs);
    public ushort div_ceil(ushort rhs);
    public ushort next_multiple_of(ushort rhs);
    public ushort? checked_next_multiple_of(ushort rhs);
    public bool is_multiple_of(ushort rhs);
    public bool is_power_of_two();
    public ushort next_power_of_two();
    public ushort? checked_next_power_of_two();
    public ubyte[2] to_be_bytes();
    public ubyte[2] to_le_bytes();
    public ubyte[2] to_ne_bytes();
    public static ushort from_be_bytes(ubyte[2] bytes);
    public static ushort from_le_bytes(ubyte[2] bytes);
    public static ushort from_ne_bytes(ubyte[2] bytes);
    public static ushort min_value();
    public static ushort max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public ushort midpoint(ushort rhs);
    public static ushort from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("u32")
@RustPrimitive("u32")
public class u32_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public i32 cast_signed();
    public i32 saturating_cast_signed();
    public i32? checked_cast_signed();
    public i32 strict_cast_signed();
    public u32 rotate_left(u32 n);
    public u32 rotate_right(u32 n);
    public u32 swap_bytes();
    public u32 reverse_bits();
    public static u32 from_be(u32 x);
    public static u32 from_le(u32 x);
    public u32 to_be();
    public u32 to_le();
    public u32? checked_add(u32 rhs);
    public u32 strict_add(u32 rhs);
    public unsafe u32 unchecked_add(u32 rhs);
    public u32? checked_add_signed(i32 rhs);
    public u32 strict_add_signed(i32 rhs);
    public u32? checked_sub(u32 rhs);
    public u32 strict_sub(u32 rhs);
    public unsafe u32 unchecked_sub(u32 rhs);
    public u32? checked_sub_signed(i32 rhs);
    public u32 strict_sub_signed(i32 rhs);
    public i32? checked_signed_diff(u32 rhs);
    public u32? checked_mul(u32 rhs);
    public u32 strict_mul(u32 rhs);
    public unsafe u32 unchecked_mul(u32 rhs);
    public u32? checked_div(u32 rhs);
    public u32 strict_div(u32 rhs);
    public u32? checked_div_euclid(u32 rhs);
    public u32 strict_div_euclid(u32 rhs);
    public u32? checked_rem(u32 rhs);
    public u32 strict_rem(u32 rhs);
    public u32? checked_rem_euclid(u32 rhs);
    public u32 strict_rem_euclid(u32 rhs);
    public u32 ilog(u32 base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(u32 base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public u32? checked_neg();
    public u32 strict_neg();
    public u32? checked_shl(u32 rhs);
    public u32 strict_shl(u32 rhs);
    public unsafe u32 unchecked_shl(u32 rhs);
    public u32 unbounded_shl(u32 rhs);
    public u32? checked_shr(u32 rhs);
    public u32 strict_shr(u32 rhs);
    public unsafe u32 unchecked_shr(u32 rhs);
    public u32 unbounded_shr(u32 rhs);
    public u32? checked_pow(u32 exp);
    public u32 strict_pow(u32 exp);
    public u32 saturating_add(u32 rhs);
    public u32 saturating_add_signed(i32 rhs);
    public u32 saturating_sub(u32 rhs);
    public u32 saturating_sub_signed(i32 rhs);
    public u32 saturating_mul(u32 rhs);
    public u32 saturating_div(u32 rhs);
    public u32 saturating_pow(u32 exp);
    public u32 wrapping_add(u32 rhs);
    public u32 wrapping_add_signed(i32 rhs);
    public u32 wrapping_sub(u32 rhs);
    public u32 wrapping_sub_signed(i32 rhs);
    public u32 wrapping_mul(u32 rhs);
    public u32 wrapping_div(u32 rhs);
    public u32 wrapping_div_euclid(u32 rhs);
    public u32 wrapping_rem(u32 rhs);
    public u32 wrapping_rem_euclid(u32 rhs);
    public u32 wrapping_neg();
    public u32 wrapping_shl(u32 rhs);
    public u32 wrapping_shr(u32 rhs);
    public u32 wrapping_pow(u32 exp);
    public (u32, bool) overflowing_add(u32 rhs);
    public (u32, bool) carrying_add(u32 rhs, bool carry);
    public (u32, bool) overflowing_add_signed(i32 rhs);
    public (u32, bool) overflowing_sub(u32 rhs);
    public (u32, bool) borrowing_sub(u32 rhs, bool borrow);
    public (u32, bool) overflowing_sub_signed(i32 rhs);
    public u32 abs_diff(u32 other);
    public (u32, bool) overflowing_mul(u32 rhs);
    public (u32, u32) carrying_mul(u32 rhs, u32 carry);
    public (u32, u32) carrying_mul_add(u32 rhs, u32 carry, u32 add);
    public (u32, bool) overflowing_div(u32 rhs);
    public (u32, bool) overflowing_div_euclid(u32 rhs);
    public (u32, bool) overflowing_rem(u32 rhs);
    public (u32, bool) overflowing_rem_euclid(u32 rhs);
    public (u32, bool) overflowing_neg();
    public (u32, bool) overflowing_shl(u32 rhs);
    public (u32, bool) overflowing_shr(u32 rhs);
    public (u32, bool) overflowing_pow(u32 exp);
    public u32 pow(u32 exp);
    public u32 isqrt();
    public u32 div_euclid(u32 rhs);
    public u32 rem_euclid(u32 rhs);
    public u32 div_ceil(u32 rhs);
    public u32 next_multiple_of(u32 rhs);
    public u32? checked_next_multiple_of(u32 rhs);
    public bool is_multiple_of(u32 rhs);
    public bool is_power_of_two();
    public u32 next_power_of_two();
    public u32? checked_next_power_of_two();
    public ubyte[4] to_be_bytes();
    public ubyte[4] to_le_bytes();
    public ubyte[4] to_ne_bytes();
    public static u32 from_be_bytes(ubyte[4] bytes);
    public static u32 from_le_bytes(ubyte[4] bytes);
    public static u32 from_ne_bytes(ubyte[4] bytes);
    public static u32 min_value();
    public static u32 max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public u32 midpoint(u32 rhs);
    public static u32 from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("u64")
@RustPrimitive("u64")
public class u64_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public long cast_signed();
    public long saturating_cast_signed();
    public long? checked_cast_signed();
    public long strict_cast_signed();
    public ulong rotate_left(u32 n);
    public ulong rotate_right(u32 n);
    public ulong swap_bytes();
    public ulong reverse_bits();
    public static ulong from_be(ulong x);
    public static ulong from_le(ulong x);
    public ulong to_be();
    public ulong to_le();
    public ulong? checked_add(ulong rhs);
    public ulong strict_add(ulong rhs);
    public unsafe ulong unchecked_add(ulong rhs);
    public ulong? checked_add_signed(long rhs);
    public ulong strict_add_signed(long rhs);
    public ulong? checked_sub(ulong rhs);
    public ulong strict_sub(ulong rhs);
    public unsafe ulong unchecked_sub(ulong rhs);
    public ulong? checked_sub_signed(long rhs);
    public ulong strict_sub_signed(long rhs);
    public long? checked_signed_diff(ulong rhs);
    public ulong? checked_mul(ulong rhs);
    public ulong strict_mul(ulong rhs);
    public unsafe ulong unchecked_mul(ulong rhs);
    public ulong? checked_div(ulong rhs);
    public ulong strict_div(ulong rhs);
    public ulong? checked_div_euclid(ulong rhs);
    public ulong strict_div_euclid(ulong rhs);
    public ulong? checked_rem(ulong rhs);
    public ulong strict_rem(ulong rhs);
    public ulong? checked_rem_euclid(ulong rhs);
    public ulong strict_rem_euclid(ulong rhs);
    public u32 ilog(ulong base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(ulong base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public ulong? checked_neg();
    public ulong strict_neg();
    public ulong? checked_shl(u32 rhs);
    public ulong strict_shl(u32 rhs);
    public unsafe ulong unchecked_shl(u32 rhs);
    public ulong unbounded_shl(u32 rhs);
    public ulong? checked_shr(u32 rhs);
    public ulong strict_shr(u32 rhs);
    public unsafe ulong unchecked_shr(u32 rhs);
    public ulong unbounded_shr(u32 rhs);
    public ulong? checked_pow(u32 exp);
    public ulong strict_pow(u32 exp);
    public ulong saturating_add(ulong rhs);
    public ulong saturating_add_signed(long rhs);
    public ulong saturating_sub(ulong rhs);
    public ulong saturating_sub_signed(long rhs);
    public ulong saturating_mul(ulong rhs);
    public ulong saturating_div(ulong rhs);
    public ulong saturating_pow(u32 exp);
    public ulong wrapping_add(ulong rhs);
    public ulong wrapping_add_signed(long rhs);
    public ulong wrapping_sub(ulong rhs);
    public ulong wrapping_sub_signed(long rhs);
    public ulong wrapping_mul(ulong rhs);
    public ulong wrapping_div(ulong rhs);
    public ulong wrapping_div_euclid(ulong rhs);
    public ulong wrapping_rem(ulong rhs);
    public ulong wrapping_rem_euclid(ulong rhs);
    public ulong wrapping_neg();
    public ulong wrapping_shl(u32 rhs);
    public ulong wrapping_shr(u32 rhs);
    public ulong wrapping_pow(u32 exp);
    public (ulong, bool) overflowing_add(ulong rhs);
    public (ulong, bool) carrying_add(ulong rhs, bool carry);
    public (ulong, bool) overflowing_add_signed(long rhs);
    public (ulong, bool) overflowing_sub(ulong rhs);
    public (ulong, bool) borrowing_sub(ulong rhs, bool borrow);
    public (ulong, bool) overflowing_sub_signed(long rhs);
    public ulong abs_diff(ulong other);
    public (ulong, bool) overflowing_mul(ulong rhs);
    public (ulong, ulong) carrying_mul(ulong rhs, ulong carry);
    public (ulong, ulong) carrying_mul_add(ulong rhs, ulong carry, ulong add);
    public (ulong, bool) overflowing_div(ulong rhs);
    public (ulong, bool) overflowing_div_euclid(ulong rhs);
    public (ulong, bool) overflowing_rem(ulong rhs);
    public (ulong, bool) overflowing_rem_euclid(ulong rhs);
    public (ulong, bool) overflowing_neg();
    public (ulong, bool) overflowing_shl(u32 rhs);
    public (ulong, bool) overflowing_shr(u32 rhs);
    public (ulong, bool) overflowing_pow(u32 exp);
    public ulong pow(u32 exp);
    public ulong isqrt();
    public ulong div_euclid(ulong rhs);
    public ulong rem_euclid(ulong rhs);
    public ulong div_ceil(ulong rhs);
    public ulong next_multiple_of(ulong rhs);
    public ulong? checked_next_multiple_of(ulong rhs);
    public bool is_multiple_of(ulong rhs);
    public bool is_power_of_two();
    public ulong next_power_of_two();
    public ulong? checked_next_power_of_two();
    public ubyte[8] to_be_bytes();
    public ubyte[8] to_le_bytes();
    public ubyte[8] to_ne_bytes();
    public static ulong from_be_bytes(ubyte[8] bytes);
    public static ulong from_le_bytes(ubyte[8] bytes);
    public static ulong from_ne_bytes(ubyte[8] bytes);
    public static ulong min_value();
    public static ulong max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public ulong midpoint(ulong rhs);
    public static ulong from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("u8")
@RustPrimitive("u8")
public class u8_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public byte cast_signed();
    public byte saturating_cast_signed();
    public byte? checked_cast_signed();
    public byte strict_cast_signed();
    public ubyte rotate_left(u32 n);
    public ubyte rotate_right(u32 n);
    public ubyte swap_bytes();
    public ubyte reverse_bits();
    public static ubyte from_be(ubyte x);
    public static ubyte from_le(ubyte x);
    public ubyte to_be();
    public ubyte to_le();
    public ubyte? checked_add(ubyte rhs);
    public ubyte strict_add(ubyte rhs);
    public unsafe ubyte unchecked_add(ubyte rhs);
    public ubyte? checked_add_signed(byte rhs);
    public ubyte strict_add_signed(byte rhs);
    public ubyte? checked_sub(ubyte rhs);
    public ubyte strict_sub(ubyte rhs);
    public unsafe ubyte unchecked_sub(ubyte rhs);
    public ubyte? checked_sub_signed(byte rhs);
    public ubyte strict_sub_signed(byte rhs);
    public byte? checked_signed_diff(ubyte rhs);
    public ubyte? checked_mul(ubyte rhs);
    public ubyte strict_mul(ubyte rhs);
    public unsafe ubyte unchecked_mul(ubyte rhs);
    public ubyte? checked_div(ubyte rhs);
    public ubyte strict_div(ubyte rhs);
    public ubyte? checked_div_euclid(ubyte rhs);
    public ubyte strict_div_euclid(ubyte rhs);
    public ubyte? checked_rem(ubyte rhs);
    public ubyte strict_rem(ubyte rhs);
    public ubyte? checked_rem_euclid(ubyte rhs);
    public ubyte strict_rem_euclid(ubyte rhs);
    public u32 ilog(ubyte base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(ubyte base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public ubyte? checked_neg();
    public ubyte strict_neg();
    public ubyte? checked_shl(u32 rhs);
    public ubyte strict_shl(u32 rhs);
    public unsafe ubyte unchecked_shl(u32 rhs);
    public ubyte unbounded_shl(u32 rhs);
    public ubyte? checked_shr(u32 rhs);
    public ubyte strict_shr(u32 rhs);
    public unsafe ubyte unchecked_shr(u32 rhs);
    public ubyte unbounded_shr(u32 rhs);
    public ubyte? checked_pow(u32 exp);
    public ubyte strict_pow(u32 exp);
    public ubyte saturating_add(ubyte rhs);
    public ubyte saturating_add_signed(byte rhs);
    public ubyte saturating_sub(ubyte rhs);
    public ubyte saturating_sub_signed(byte rhs);
    public ubyte saturating_mul(ubyte rhs);
    public ubyte saturating_div(ubyte rhs);
    public ubyte saturating_pow(u32 exp);
    public ubyte wrapping_add(ubyte rhs);
    public ubyte wrapping_add_signed(byte rhs);
    public ubyte wrapping_sub(ubyte rhs);
    public ubyte wrapping_sub_signed(byte rhs);
    public ubyte wrapping_mul(ubyte rhs);
    public ubyte wrapping_div(ubyte rhs);
    public ubyte wrapping_div_euclid(ubyte rhs);
    public ubyte wrapping_rem(ubyte rhs);
    public ubyte wrapping_rem_euclid(ubyte rhs);
    public ubyte wrapping_neg();
    public ubyte wrapping_shl(u32 rhs);
    public ubyte wrapping_shr(u32 rhs);
    public ubyte wrapping_pow(u32 exp);
    public (ubyte, bool) overflowing_add(ubyte rhs);
    public (ubyte, bool) carrying_add(ubyte rhs, bool carry);
    public (ubyte, bool) overflowing_add_signed(byte rhs);
    public (ubyte, bool) overflowing_sub(ubyte rhs);
    public (ubyte, bool) borrowing_sub(ubyte rhs, bool borrow);
    public (ubyte, bool) overflowing_sub_signed(byte rhs);
    public ubyte abs_diff(ubyte other);
    public (ubyte, bool) overflowing_mul(ubyte rhs);
    public (ubyte, ubyte) carrying_mul(ubyte rhs, ubyte carry);
    public (ubyte, ubyte) carrying_mul_add(ubyte rhs, ubyte carry, ubyte add);
    public (ubyte, bool) overflowing_div(ubyte rhs);
    public (ubyte, bool) overflowing_div_euclid(ubyte rhs);
    public (ubyte, bool) overflowing_rem(ubyte rhs);
    public (ubyte, bool) overflowing_rem_euclid(ubyte rhs);
    public (ubyte, bool) overflowing_neg();
    public (ubyte, bool) overflowing_shl(u32 rhs);
    public (ubyte, bool) overflowing_shr(u32 rhs);
    public (ubyte, bool) overflowing_pow(u32 exp);
    public ubyte pow(u32 exp);
    public ubyte isqrt();
    public ubyte div_euclid(ubyte rhs);
    public ubyte rem_euclid(ubyte rhs);
    public ubyte div_ceil(ubyte rhs);
    public ubyte next_multiple_of(ubyte rhs);
    public ubyte? checked_next_multiple_of(ubyte rhs);
    public bool is_multiple_of(ubyte rhs);
    public bool is_power_of_two();
    public ubyte next_power_of_two();
    public ubyte? checked_next_power_of_two();
    public ubyte[1] to_be_bytes();
    public ubyte[1] to_le_bytes();
    public ubyte[1] to_ne_bytes();
    public static ubyte from_be_bytes(ubyte[1] bytes);
    public static ubyte from_le_bytes(ubyte[1] bytes);
    public static ubyte from_ne_bytes(ubyte[1] bytes);
    public static ubyte min_value();
    public static ubyte max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public ubyte midpoint(ubyte rhs);
    public bool is_ascii();
    public ubyte to_ascii_uppercase();
    public ubyte to_ascii_lowercase();
    public bool eq_ignore_ascii_case(&ubyte other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
    public bool is_ascii_alphabetic();
    public bool is_ascii_uppercase();
    public bool is_ascii_lowercase();
    public bool is_ascii_alphanumeric();
    public bool is_ascii_digit();
    public bool is_ascii_hexdigit();
    public bool is_ascii_punctuation();
    public bool is_ascii_graphic();
    public bool is_ascii_whitespace();
    public bool is_ascii_control();
    public EscapeDefault escape_ascii();
    public static ubyte from_str_radix(&String src, u32 radix) throws ParseIntError;
}

public type uid_t = u32;


@rust("usize")
@RustPrimitive("usize")
public class usize_methods {
    public u32 count_ones();
    public u32 count_zeros();
    public u32 leading_zeros();
    public u32 trailing_zeros();
    public u32 leading_ones();
    public u32 trailing_ones();
    public int cast_signed();
    public int saturating_cast_signed();
    public int? checked_cast_signed();
    public int strict_cast_signed();
    public uint rotate_left(u32 n);
    public uint rotate_right(u32 n);
    public uint swap_bytes();
    public uint reverse_bits();
    public static uint from_be(uint x);
    public static uint from_le(uint x);
    public uint to_be();
    public uint to_le();
    public uint? checked_add(uint rhs);
    public uint strict_add(uint rhs);
    public unsafe uint unchecked_add(uint rhs);
    public uint? checked_add_signed(int rhs);
    public uint strict_add_signed(int rhs);
    public uint? checked_sub(uint rhs);
    public uint strict_sub(uint rhs);
    public unsafe uint unchecked_sub(uint rhs);
    public uint? checked_sub_signed(int rhs);
    public uint strict_sub_signed(int rhs);
    public int? checked_signed_diff(uint rhs);
    public uint? checked_mul(uint rhs);
    public uint strict_mul(uint rhs);
    public unsafe uint unchecked_mul(uint rhs);
    public uint? checked_div(uint rhs);
    public uint strict_div(uint rhs);
    public uint? checked_div_euclid(uint rhs);
    public uint strict_div_euclid(uint rhs);
    public uint? checked_rem(uint rhs);
    public uint strict_rem(uint rhs);
    public uint? checked_rem_euclid(uint rhs);
    public uint strict_rem_euclid(uint rhs);
    public u32 ilog(uint base);
    public u32 ilog2();
    public u32 ilog10();
    public u32? checked_ilog(uint base);
    public u32? checked_ilog2();
    public u32? checked_ilog10();
    public uint? checked_neg();
    public uint strict_neg();
    public uint? checked_shl(u32 rhs);
    public uint strict_shl(u32 rhs);
    public unsafe uint unchecked_shl(u32 rhs);
    public uint unbounded_shl(u32 rhs);
    public uint? checked_shr(u32 rhs);
    public uint strict_shr(u32 rhs);
    public unsafe uint unchecked_shr(u32 rhs);
    public uint unbounded_shr(u32 rhs);
    public uint? checked_pow(u32 exp);
    public uint strict_pow(u32 exp);
    public uint saturating_add(uint rhs);
    public uint saturating_add_signed(int rhs);
    public uint saturating_sub(uint rhs);
    public uint saturating_sub_signed(int rhs);
    public uint saturating_mul(uint rhs);
    public uint saturating_div(uint rhs);
    public uint saturating_pow(u32 exp);
    public uint wrapping_add(uint rhs);
    public uint wrapping_add_signed(int rhs);
    public uint wrapping_sub(uint rhs);
    public uint wrapping_sub_signed(int rhs);
    public uint wrapping_mul(uint rhs);
    public uint wrapping_div(uint rhs);
    public uint wrapping_div_euclid(uint rhs);
    public uint wrapping_rem(uint rhs);
    public uint wrapping_rem_euclid(uint rhs);
    public uint wrapping_neg();
    public uint wrapping_shl(u32 rhs);
    public uint wrapping_shr(u32 rhs);
    public uint wrapping_pow(u32 exp);
    public (uint, bool) overflowing_add(uint rhs);
    public (uint, bool) carrying_add(uint rhs, bool carry);
    public (uint, bool) overflowing_add_signed(int rhs);
    public (uint, bool) overflowing_sub(uint rhs);
    public (uint, bool) borrowing_sub(uint rhs, bool borrow);
    public (uint, bool) overflowing_sub_signed(int rhs);
    public uint abs_diff(uint other);
    public (uint, bool) overflowing_mul(uint rhs);
    public (uint, uint) carrying_mul(uint rhs, uint carry);
    public (uint, uint) carrying_mul_add(uint rhs, uint carry, uint add);
    public (uint, bool) overflowing_div(uint rhs);
    public (uint, bool) overflowing_div_euclid(uint rhs);
    public (uint, bool) overflowing_rem(uint rhs);
    public (uint, bool) overflowing_rem_euclid(uint rhs);
    public (uint, bool) overflowing_neg();
    public (uint, bool) overflowing_shl(u32 rhs);
    public (uint, bool) overflowing_shr(u32 rhs);
    public (uint, bool) overflowing_pow(u32 exp);
    public uint pow(u32 exp);
    public uint isqrt();
    public uint div_euclid(uint rhs);
    public uint rem_euclid(uint rhs);
    public uint div_ceil(uint rhs);
    public uint next_multiple_of(uint rhs);
    public uint? checked_next_multiple_of(uint rhs);
    public bool is_multiple_of(uint rhs);
    public bool is_power_of_two();
    public uint next_power_of_two();
    public uint? checked_next_power_of_two();
    public ubyte[8] to_be_bytes();
    public ubyte[8] to_le_bytes();
    public ubyte[8] to_ne_bytes();
    public static uint from_be_bytes(ubyte[8] bytes);
    public static uint from_le_bytes(ubyte[8] bytes);
    public static uint from_ne_bytes(ubyte[8] bytes);
    public static uint min_value();
    public static uint max_value();
    public Target widen<Target>();
    public T saturating_cast<T>();
    public T wrapping_cast<T>();
    public T? checked_cast<T>();
    public T strict_cast<T>();
    public unsafe T unchecked_cast<T>();
    public uint midpoint(uint rhs);
    public static uint from_str_radix(&String src, u32 radix) throws ParseIntError;
}

@rust("std::env::var")
public String var<K>(K key) throws VarError;

@rust("std::env::var_os")
public OsString? var_os<K>(K key);

@rust("std::env::vars")
public Vars vars();

@rust("std::env::vars_os")
public VarsOs vars_os();

@rust("std::fs::write")
public void write<P, C>(P path, C contents) throws Error;

@rust("std::intrinsics::write_box_via_move")
public MaybeUninit<T> write_box_via_move<T>(MaybeUninit<T> b, T x);

@rust("std::thread::yield_now")
public void yield_now();
