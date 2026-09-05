// juxc rust.std stub cache-version 12
// bindgen — generated from 2 rustdoc JSON crate(s) (format_version 58)

package rust.std;

public const String ARCH;

/** An error returned by [`LocalKey::try_with`](struct.LocalKey.html#method.try_with). */
@rust("std::thread::local::AccessError")
public class AccessError {
}

/** An iterator over [`Path`] and its ancestors. */
@rust("std::path::Ancestors")
public class Ancestors {
}

/** This enum represent one control message of variable type. */
@rust("std::os::unix::net::ancillary::AncillaryData")
public enum AncillaryData {
    ScmRights(ScmRights), ScmCredentials(ScmCredentials)
}

/** The error type which is returned from parsing the type a control message. */
@rust("std::os::unix::net::ancillary::AncillaryError")
public enum AncillaryError {
    Unknown
}

/** A thread-safe reference-counting pointer. 'Arc' stands for 'Atomically */
@rust("std::sync::Arc")
public class Arc<T, A> {
    public Arc(T data);
    public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T data);
    public static Pin<T> try_pin(T data) throws AllocError;
    public static T try_new(T data) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static U map<U>(Self this, (T) -> U f);
    public static TryType try_map<R>(Self this, (T) -> R f);
    public static T new_in(T data, A alloc);
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static T new_cyclic_in<F>((Weak<T, A>) -> T data_fn, A alloc);
    public static Pin<T> pin_in(T data, A alloc);
    public static Pin<T> try_pin_in(T data, A alloc) throws AllocError;
    public static T try_new_in(T data, A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static T try_unwrap(Self this) throws Self;
    public static T? into_inner(Self this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public static MaybeUninit<T>[] new_uninit_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] new_zeroed_slice_in(uint len, A alloc);
    public T[] into_array() throws Self;
    public unsafe T assume_init();
    public static T clone_from_ref(&T value);
    public static T try_clone_from_ref(&T value) throws AllocError;
    public static T clone_from_ref_in(&T value, A alloc);
    public static T try_clone_from_ref_in(&T value, A alloc) throws AllocError;
    public static unsafe Self from_raw(T* ptr);
    public static T* into_raw(Self this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    public static A allocator(&Self this);
    public static Tuple<T*, A> into_raw_with_allocator(Self this);
    public static T* as_ptr(&Self this);
    public static unsafe Self from_raw_in(T* ptr, A alloc);
    public static Weak<T, A> downgrade(&Self this);
    public static uint weak_count(&Self this);
    public static uint strong_count(&Self this);
    public static unsafe void increment_strong_count_in(T* ptr, A alloc);
    public static unsafe void decrement_strong_count_in(T* ptr, A alloc);
    public static bool ptr_eq(&Self this, &Self other);
    public static T make_mut(&Self this);
    public static T unwrap_or_clone(Self this);
    public static T? get_mut(&Self this);
    public static unsafe T get_mut_unchecked(&Self this);
    public static bool is_unique(&Self this);
    public T downcast<T>() throws Self;
    public unsafe T downcast_unchecked<T>();
}

/** An iterator over the arguments of a process, yielding a [`String`] value for */
@rust("std::env::Args")
public class Args {
}

/** An iterator over the arguments of a process, yielding an [`OsString`] value */
@rust("std::env::ArgsOs")
public class ArgsOs {
}

/** A trait to borrow the file descriptor from an underlying object. */
@rust("std::os::fd::owned::AsFd")
public interface AsFd {
    public BorrowedFd as_fd();
}

/** A trait to borrow the handle from an underlying object. */
@rust("std::os::windows::io::handle::AsHandle")
public interface AsHandle {
    public BorrowedHandle as_handle();
}

/** A trait to extract the raw file descriptor from an underlying object. */
@rust("std::os::fd::raw::AsRawFd")
public interface AsRawFd {
    public RawFd as_raw_fd();
}

/** Extracts raw handles. */
@rust("std::os::windows::io::raw::AsRawHandle")
public interface AsRawHandle {
    public RawHandle as_raw_handle();
}

/** Extracts raw sockets. */
@rust("std::os::windows::io::raw::AsRawSocket")
public interface AsRawSocket {
    public RawSocket as_raw_socket();
}

/** A trait to borrow the socket from an underlying object. */
@rust("std::os::windows::io::socket::AsSocket")
public interface AsSocket {
    public BorrowedSocket as_socket();
}

/** Extension methods for ASCII-subset only operations. */
@rust("std::ascii::AsciiExt")
public interface AsciiExt {
    public bool is_ascii();
    public Owned to_ascii_uppercase();
    public Owned to_ascii_lowercase();
    public bool eq_ignore_ascii_case(&Self other);
    @MutSelf public void make_ascii_uppercase();
    @MutSelf public void make_ascii_lowercase();
}

/** An ordered map based on a [B-Tree]. */
@rust("std::collections::BTreeMap")
@RustIndexRef
public class BTreeMap<K, V, A> {
    public BTreeMap();
    @MutSelf public void clear();
    public static Map<K, V> new_in(A alloc);
    public V? get<Q>(&Q key);
    public Tuple<K, V>? get_key_value<Q>(&Q k);
    public Tuple<K, V>? first_key_value();
    @MutSelf public OccupiedEntry<K, V, A>? first_entry();
    @MutSelf public Tuple<K, V>? pop_first();
    public Tuple<K, V>? last_key_value();
    @MutSelf public OccupiedEntry<K, V, A>? last_entry();
    @MutSelf public Tuple<K, V>? pop_last();
    public bool contains_key<Q>(&Q key);
    @MutSelf public V? get_mut<Q>(&Q key);
    @MutSelf public V? insert(K key, V value);
    @MutSelf public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf public V? remove<Q>(&Q key);
    @MutSelf public Tuple<K, V>? remove_entry<Q>(&Q key);
    @MutSelf public void retain<F>((K, V) -> bool f);
    @MutSelf public void append(&Self other);
    @MutSelf public void merge(Self other, (K, V, V) -> V conflict);
    public Range<K, V> range<T, R>(R range);
    @MutSelf public RangeMut<K, V> range_mut<T, R>(R range);
    @MutSelf public Entry<K, V, A> entry(K key);
    @MutSelf public Self split_off<Q>(&Q key);
    @MutSelf public ExtractIf<K, V, R, F, A> extract_if<F, R>(R range, (K, V) -> bool pred);
    public IntoKeys<K, V, A> into_keys();
    public IntoValues<K, V, A> into_values();
    public Iter<K, V> iter();
    @MutSelf public IterMut<K, V> iter_mut();
    public Keys<K, V> keys();
    public Values<K, V> values();
    @MutSelf public ValuesMut<K, V> values_mut();
    public uint len();
    public bool is_empty();
    public Cursor<K, V> lower_bound<Q>(Bound<Q> bound);
    @MutSelf public CursorMut<K, V, A> lower_bound_mut<Q>(Bound<Q> bound);
    public Cursor<K, V> upper_bound<Q>(Bound<Q> bound);
    @MutSelf public CursorMut<K, V, A> upper_bound_mut<Q>(Bound<Q> bound);
}

/** An ordered set based on a B-Tree. */
@rust("std::collections::BTreeSet")
public class BTreeSet<T, A> {
    public BTreeSet();
    public static Set<T> new_in(A alloc);
    public Range<T> range<K, R>(R range);
    public Difference<T, A> difference(&Set<T> other);
    public SymmetricDifference<T> symmetric_difference(&Set<T> other);
    public Intersection<T, A> intersection(&Set<T> other);
    public Union<T> union(&Set<T> other);
    @MutSelf public void clear();
    public bool contains<Q>(&Q value);
    public T? get<Q>(&Q value);
    public bool is_disjoint(&Set<T> other);
    public bool is_subset(&Set<T> other);
    public bool is_superset(&Set<T> other);
    public T? first();
    public T? last();
    @MutSelf public T? pop_first();
    @MutSelf public T? pop_last();
    @MutSelf public bool insert(T value);
    @MutSelf public T? replace(T value);
    @MutSelf public T get_or_insert(T value);
    @MutSelf public T get_or_insert_with<Q, F>(&Q value, (Q) -> T f);
    @MutSelf public Entry<T, A> entry(T value);
    @MutSelf public bool remove<Q>(&Q value);
    @MutSelf public T? take<Q>(&Q value);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void append(&Self other);
    @MutSelf public Self split_off<Q>(&Q value);
    @MutSelf public ExtractIf<T, R, F, A> extract_if<F, R>(R range, (T) -> bool pred);
    public Iter<T> iter();
    public uint len();
    public bool is_empty();
    public Cursor<T> lower_bound<Q>(Bound<Q> bound);
    @MutSelf public CursorMut<T, A> lower_bound_mut<Q>(Bound<Q> bound);
    public Cursor<T> upper_bound<Q>(Bound<Q> bound);
    @MutSelf public CursorMut<T, A> upper_bound_mut<Q>(Bound<Q> bound);
}

/** A captured OS thread stack backtrace. */
@rust("std::backtrace::Backtrace")
public class Backtrace {
    public static Backtrace capture();
    public static Backtrace force_capture();
    public static Backtrace disabled();
    public BacktraceStatus status();
    public BacktraceFrame[] frames();
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
public enum BacktraceStyle {
    Short, Full, Off
}

/** A barrier enables multiple threads to synchronize the beginning */
@rust("std::sync::barrier::Barrier")
public class Barrier {
    public Barrier(uint n);
    public BarrierWaitResult wait();
}

/** A `BarrierWaitResult` is returned by [`Barrier::wait()`] when all threads */
@rust("std::sync::barrier::BarrierWaitResult")
public class BarrierWaitResult {
    public bool is_leader();
}

/** A priority queue implemented with a binary heap. */
@rust("std::collections::BinaryHeap")
public class BinaryHeap<T, A> {
    public BinaryHeap();
    public static BinaryHeap<T> with_capacity(uint capacity);
    public static BinaryHeap<T, A> new_in(A alloc);
    public static BinaryHeap<T, A> with_capacity_in(uint capacity, A alloc);
    public static unsafe BinaryHeap<T, A> from_raw_vec(List<T> vec);
    @MutSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public T? pop();
    @MutSelf public T? pop_if((T) -> bool predicate);
    @MutSelf public void push(T item);
    public List<T> into_sorted_vec();
    @MutSelf public void append(&Self other);
    @MutSelf public DrainSorted<T, A> drain_sorted();
    @MutSelf public void retain<F>((T) -> bool f);
    public Iter<T> iter();
    public IntoIterSorted<T, A> into_iter_sorted();
    public T? peek();
    public uint capacity();
    @MutSelf public void reserve_exact(uint additional);
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve_exact(uint additional) throws TryReserveError;
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    public T[] as_slice();
    @MutSelf public unsafe T[] as_mut_slice();
    public List<T> into_vec();
    public A allocator();
    public uint len();
    public bool is_empty();
    @MutSelf public Drain<T, A> drain();
    @MutSelf public void clear();
}

/** A borrowed file descriptor. */
@rust("std::os::fd::owned::BorrowedFd")
public class BorrowedFd {
    public static unsafe Self borrow_raw(RawFd fd);
    public OwnedFd try_clone_to_owned() throws Error;
}

/** A borrowed handle. */
@rust("std::os::windows::io::handle::BorrowedHandle")
public class BorrowedHandle {
    public static unsafe Self borrow_raw(RawHandle handle);
    public OwnedHandle try_clone_to_owned() throws Error;
}

/** A borrowed socket. */
@rust("std::task::Wake")
public class BorrowedSocket {
    public static unsafe Self borrow_raw(RawSocket socket);
    public OwnedSocket try_clone_to_owned() throws Error;
}

/** A pointer type that uniquely owns a heap allocation of type `T`. */
@rust("std::boxed::Box")
public class Box<T, A> {
    public Box(T x);
    public T downcast<T>() throws Self;
    public unsafe T downcast_unchecked<T>();
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static Pin<T> pin(T x);
    public static Self try_new(T x) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static U map<U>(Self this, (T) -> U f);
    public static TryType try_map<R>(Self this, (T) -> R f);
    public static Self new_in(T x, A alloc);
    public static Self try_new_in(T x, A alloc) throws AllocError;
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static Pin<Self> pin_in(T x, A alloc);
    public static T[] into_boxed_slice(Self boxed);
    public static T into_inner(Self boxed);
    public static Tuple<T, MaybeUninit<T>> take(Self boxed);
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
    public T[] into_array() throws Self;
    public unsafe T assume_init();
    public static T write(Self boxed, T value);
    public static unsafe Self from_raw(T* raw);
    public static unsafe Self from_non_null(NonNull<T> ptr);
    public static T* into_raw(Self b);
    public static NonNull<T> into_non_null(Self b);
    public static unsafe Self from_raw_in(T* raw, A alloc);
    public static unsafe Self from_non_null_in(NonNull<T> raw, A alloc);
    public static Tuple<T*, A> into_raw_with_allocator(Self b);
    public static Tuple<NonNull<T>, A> into_non_null_with_allocator(Self b);
    public static T* as_mut_ptr(&Self b);
    public static T* as_ptr(&Self b);
    public static A allocator(&Self b);
    public static T leak(Self b);
    public static Pin<Self> into_pin(Self boxed);
}

/** A `BufRead` is a type of `Read`er which has an internal buffer, allowing it */
@rust("std::io::BufRead")
public interface BufRead {
    @MutSelf public ubyte[] fill_buf() throws Error;
    @MutSelf public void consume(uint amount);
    @MutSelf public bool has_data_left() throws Error;
    @MutSelf public uint read_until(ubyte byte, &List<ubyte> buf) throws Error;
    @MutSelf public uint skip_until(ubyte byte) throws Error;
    @MutSelf public uint read_line(&String buf) throws Error;
    public Split<Self> split(ubyte byte);
    public Lines<Self> lines();
}

/** The `BufReader<R>` struct adds buffering to any reader. */
@rust("std::io::buffered::bufreader::BufReader")
public class BufReader<R> {
    public BufReader(R inner);
    public static BufReader<R> with_capacity(uint capacity, R inner);
    @MutSelf public ubyte[] peek(uint n) throws Error;
    public R get_ref();
    @MutSelf public R get_mut();
    public ubyte[] buffer();
    public uint capacity();
    public R into_inner();
    @MutSelf public void seek_relative(long offset) throws Error;
}

/** Wraps a writer and buffers its output. */
@rust("std::io::buffered::bufwriter::BufWriter")
public class BufWriter<W> {
    public BufWriter(W inner);
    public static BufWriter<W> with_capacity(uint capacity, W inner);
    public W into_inner() throws IntoInnerError<BufWriter<W>>;
    public Tuple<W, Result<List<ubyte>, WriterPanicked>> into_parts();
    public W get_ref();
    @MutSelf public W get_mut();
    public ubyte[] buffer();
    public uint capacity();
}

/** Thread factory, which can be used in order to configure the properties of */
@rust("std::thread::builder::Builder")
public class Builder {
    public Builder();
    public Builder name(String name);
    public Builder stack_size(uint size);
    public Builder no_hooks();
    public JoinHandle<T> spawn<F, T>(() -> T f) throws Error;
    public unsafe JoinHandle<T> spawn_unchecked<F, T>(() -> T f) throws Error;
    public ScopedJoinHandle<T> spawn_scoped<F, T>(&Scope scope, () -> T f) throws Error;
}

/** A wrapper for `Vec<u8>` representing a human-readable string that's conventionally, but not */
@rust("std::bstr::ByteString")
public class ByteString {
}

/** An iterator over `u8` values of a reader. */
@rust("std::io::Bytes")
public class Bytes<R> {
}

/** A type representing an owned, C-compatible, nul-terminated string with no nul bytes in the */
@rust("std::ffi::c_str::CString")
public class CString {
    public CString(T t) throws NulError;
    public static unsafe Self from_vec_unchecked(List<ubyte> v);
    public static unsafe CString from_raw(c_char* ptr);
    public c_char* into_raw();
    public String into_string() throws IntoStringError;
    public List<ubyte> into_bytes();
    public List<ubyte> into_bytes_with_nul();
    public ubyte[] as_bytes();
    public ubyte[] as_bytes_with_nul();
    public CStr as_c_str();
    public CStr into_boxed_c_str();
    public static unsafe Self from_vec_with_nul_unchecked(List<ubyte> v);
    public static Self from_vec_with_nul(List<ubyte> v) throws FromVecWithNulError;
}

/** Representation of a running or exited child process. */
@rust("std::process::Child")
public class Child {
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
    public PidFd pidfd() throws Error;
    public PidFd into_pidfd() throws Self;
}

/** A handle to a child process's stderr. */
@rust("std::process::ChildStderr")
public class ChildStderr {
}

/** A handle to a child process's standard input (stdin). */
@rust("std::process::ChildStdin")
public class ChildStdin {
}

/** A handle to a child process's standard output (stdout). */
@rust("std::process::ChildStdout")
public class ChildStdout {
}

/** A process builder, providing fine-grained control */
@rust("std::process::Command")
public class Command {
    public Command(S program);
    @MutSelf public Command arg<S>(S arg);
    @MutSelf public Command args<I, S>(I args);
    @MutSelf public Command env<K, V>(K key, V val);
    @MutSelf public Command envs<I, K, V>(I vars);
    @MutSelf public Command env_remove<K>(K key);
    @MutSelf public Command env_clear();
    @MutSelf public Command current_dir<P>(P dir);
    @MutSelf public Command stdin<T>(T cfg);
    @MutSelf public Command stdout<T>(T cfg);
    @MutSelf public Command stderr<T>(T cfg);
    @MutSelf public Child spawn() throws Error;
    @MutSelf public Output output() throws Error;
    @MutSelf public ExitStatus status() throws Error;
    public OsStr get_program();
    public CommandArgs get_args();
    public CommandEnvs get_envs();
    public CommandResolvedEnvs get_resolved_envs();
    public Path? get_current_dir();
    public bool get_env_clear();
}

/** An iterator over the command arguments. */
@rust("std::process::CommandArgs")
public class CommandArgs {
}

/** An iterator over the command environment variables. */
@rust("std::process::CommandEnvs")
public class CommandEnvs {
}

/** Os-specific extensions for [`Command`] */
@rust("std::os::linux::process::CommandExt")
public interface CommandExt {
    @MutSelf public Command create_pidfd(bool val);
}

/** An iterator over the fully resolved environment variables. */
@rust("std::sys::process::env::CommandResolvedEnvs")
public class CommandResolvedEnvs {
}

/** A single component of a path. */
@rust("std::path::Component")
public enum Component {
    Prefix(PrefixComponent), RootDir, CurDir, ParentDir, Normal(OsStr)
}

/** An iterator over the [`Component`]s of a [`Path`]. */
@rust("std::path::Components")
public class Components {
    public Path as_path();
}

/** Helper trait for [`[T]::concat`](slice::concat). */
@rust("std::slice::Concat")
public interface Concat<Item> {
    public Output concat(&Self slice);
}

/** A Condition Variable */
@rust("std::sync::nonpoison::condvar::Condvar")
public class Condvar {
    public Condvar();
    public void wait<T>(&MutexGuard<T> guard);
    public void wait_while<T, F>(&MutexGuard<T> guard, (T) -> bool condition);
    public WaitTimeoutResult wait_timeout<T>(&MutexGuard<T> guard, Duration dur);
    public WaitTimeoutResult wait_timeout_while<T, F>(&MutexGuard<T> guard, Duration dur, (T) -> bool condition);
    public void notify_one();
    public void notify_all();
}

/** A clone-on-write smart pointer. */
@rust("std::borrow::Cow")
public enum Cow<B> {
    Borrowed(B), Owned(Owned)
}

/** A cursor over a `BTreeMap`. */
@rust("std::collections::Cursor")
public class Cursor<K, V> {
    @MutSelf public Tuple<K, V>? next();
    @MutSelf public Tuple<K, V>? prev();
    public Tuple<K, V>? peek_next();
    public Tuple<K, V>? peek_prev();
}

/** A cursor over a `BTreeMap` with editing operations. */
@rust("std::collections::CursorMut")
public class CursorMut<K, V, A> {
    @MutSelf public Tuple<K, V>? next();
    @MutSelf public Tuple<K, V>? prev();
    @MutSelf public Tuple<K, V>? peek_next();
    @MutSelf public Tuple<K, V>? peek_prev();
    public Cursor<K, V> as_cursor();
    public unsafe CursorMutKey<K, V, A> with_mutable_key();
    @MutSelf public unsafe void insert_after_unchecked(K key, V value);
    @MutSelf public unsafe void insert_before_unchecked(K key, V value);
    @MutSelf public void insert_after(K key, V value) throws UnorderedKeyError;
    @MutSelf public void insert_before(K key, V value) throws UnorderedKeyError;
    @MutSelf public Tuple<K, V>? remove_next();
    @MutSelf public Tuple<K, V>? remove_prev();
}

/** A cursor over a `BTreeMap` with editing operations, and which allows */
@rust("std::collections::CursorMutKey")
public class CursorMutKey<K, V, A> {
    @MutSelf public Tuple<K, V>? next();
    @MutSelf public Tuple<K, V>? prev();
    @MutSelf public Tuple<K, V>? peek_next();
    @MutSelf public Tuple<K, V>? peek_prev();
    public Cursor<K, V> as_cursor();
    @MutSelf public unsafe void insert_after_unchecked(K key, V value);
    @MutSelf public unsafe void insert_before_unchecked(K key, V value);
    @MutSelf public void insert_after(K key, V value) throws UnorderedKeyError;
    @MutSelf public void insert_before(K key, V value) throws UnorderedKeyError;
    @MutSelf public Tuple<K, V>? remove_next();
    @MutSelf public Tuple<K, V>? remove_prev();
}

public const String DLL_EXTENSION;

public const String DLL_PREFIX;

public const String DLL_SUFFIX;

/** The default [`Hasher`] used by [`RandomState`]. */
@rust("std::hash::random::DefaultHasher")
public class DefaultHasher {
    public DefaultHasher();
}

/** A lazy iterator producing elements in the difference of `BTreeSet`s. */
@rust("std::collections::Difference")
public class Difference<T, A> {
}

/** An object providing access to a directory on the filesystem. */
@rust("std::fs::Dir")
public class Dir {
    public static Self open<P>(P path) throws Error;
    public File open_file<P>(P path) throws Error;
    public Metadata metadata() throws Error;
}

/** A builder used to create directories in various manners. */
@rust("std::fs::DirBuilder")
public class DirBuilder {
    public DirBuilder();
    @MutSelf public Self recursive(bool recursive);
    public void create<P>(P path) throws Error;
}

/** Unix-specific extensions to [`fs::DirBuilder`]. */
@rust("std::os::unix::fs::DirBuilderExt")
public interface DirBuilderExt {
    @MutSelf public Self mode(u32 mode);
}

/** Entries returned by the [`ReadDir`] iterator. */
@rust("std::fs::DirEntry")
public class DirEntry {
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
    public OsStr file_name_ref();
}

/** Helper struct for safely printing paths with [`format!`] and `{}`. */
@rust("std::path::Display")
public class Display {
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Drain")
public class Drain<T, A> {
    public A allocator();
}

/** A draining iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::DrainSorted")
public class DrainSorted<T, A> {
    public A allocator();
}

public const String EXE_EXTENSION;

public const String EXE_SUFFIX;

/** Iterator returned by [`OsStrExt::encode_wide`]. */
@rust("std::os::windows::ffi::EncodeWide")
public class EncodeWide {
}

/** A view into a single entry in a map, which may either be vacant or occupied. */
@rust("std::collections::Entry")
public enum Entry<K, V, A> {
    Vacant(VacantEntry<K, V, A>), Occupied(OccupiedEntry<K, V, A>)
}

/** The error type for I/O operations of the [`Read`], [`Write`], [`Seek`], and */
@rust("std::io::error::Error")
public class Error {
    public Error(ErrorKind kind, E error);
    public static Error other<E>(E error);
    public static Error last_os_error();
    public static Error from_raw_os_error(RawOsError code);
    public RawOsError? raw_os_error();
    public Error? get_ref();
    @MutSelf public Error? get_mut();
    public Error? into_inner();
    public E downcast<E>() throws Self;
    public ErrorKind kind();
}

/** This type represents the status code the current process can return */
@rust("std::process::ExitCode")
public class ExitCode {
    public never exit_process();
}

/** Windows-specific extensions to [`process::ExitCode`]. */
@rust("std::os::windows::process::ExitCodeExt")
public interface ExitCodeExt {
    public Self from_raw(u32 raw);
}

/** Describes the result of a process after it has terminated. */
@rust("std::process::ExitStatus")
public class ExitStatus {
    public void exit_ok() throws ExitStatusError;
    public bool success();
    public i32? code();
}

/** Describes the result of a process after it has failed */
@rust("std::process::ExitStatusError")
public class ExitStatusError {
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

/** This `struct` is created by the [`extract_if`] method on [`BTreeMap`]. */
@rust("std::collections::ExtractIf")
public class ExtractIf<K, V, R, F, A> {
}

public const String FAMILY;

/** An object providing access to an open file on the filesystem. */
@rust("std::fs::File")
public class File {
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
    public uint read_at(ubyte[] buf, ulong offset) throws Error;
    public uint read_vectored_at(IoSliceMut[] bufs, ulong offset) throws Error;
    public void read_exact_at(ubyte[] buf, ulong offset) throws Error;
    public void read_buf_at(BorrowedCursor<ubyte> buf, ulong offset) throws Error;
    public void read_buf_exact_at(BorrowedCursor<ubyte> buf, ulong offset) throws Error;
    public uint write_at(ubyte[] buf, ulong offset) throws Error;
    public uint write_vectored_at(IoSlice[] bufs, ulong offset) throws Error;
    public void write_all_at(ubyte[] buf, ulong offset) throws Error;
}

/** Representation of the various timestamps on a file. */
@rust("std::fs::FileTimes")
public class FileTimes {
    public FileTimes();
    public Self set_accessed(SystemTime t);
    public Self set_modified(SystemTime t);
}

/** OS-specific extensions to [`fs::FileTimes`]. */
@rust("std::os::darwin::fs::FileTimesExt")
public interface FileTimesExt {
    public Self set_created(SystemTime t);
}

/** A structure representing a type of file with accessors for each file type. */
@rust("std::fs::FileType")
public class FileType {
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

/** Trait for types that can be converted from a fixed-size byte array with a specified endianness */
@rust("std::str::SplitInclusive")
public interface FromEndianBytes {
}

/** A trait to express the ability to construct an object from a raw file */
@rust("std::os::fd::raw::FromRawFd")
public interface FromRawFd {
    public unsafe Self from_raw_fd(RawFd fd);
}

/** Constructs I/O objects from raw handles. */
@rust("std::os::windows::io::raw::FromRawHandle")
public interface FromRawHandle {
    public unsafe Self from_raw_handle(RawHandle handle);
}

/** Creates I/O objects from raw sockets. */
@rust("std::os::windows::io::raw::FromRawSocket")
public interface FromRawSocket {
    public unsafe Self from_raw_socket(RawSocket sock);
}

/** A possible error value when converting a `String` from a UTF-16 byte slice. */
@rust("std::string::FromUtf16Error")
public class FromUtf16Error {
}

/** A possible error value when converting a `String` from a UTF-8 byte vector. */
@rust("std::string::FromUtf8Error")
public class FromUtf8Error {
    public ubyte[] as_bytes();
    public String into_utf8_lossy();
    public List<ubyte> into_bytes();
    public Utf8Error utf8_error();
}

/** An error indicating that a nul byte was not in the expected position. */
@rust("std::ffi::c_str::FromVecWithNulError")
public class FromVecWithNulError {
    public ubyte[] as_bytes();
    public List<ubyte> into_bytes();
}

/** The global memory allocator. */
@rust("std::alloc::Global")
public class Global {
}

/** An owned container for `HANDLE` object, closing them on Drop. */
@rust("std::sys::pal::windows::handle::Handle")
public class Handle {
}

/** FFI type for handles in return values or out parameters, where `INVALID_HANDLE_VALUE` is used */
@rust("std::os::windows::io::handle::HandleOrInvalid")
public class HandleOrInvalid {
    public static unsafe Self from_raw_handle(RawHandle handle);
}

/** FFI type for handles in return values or out parameters, where `NULL` is used */
@rust("std::os::windows::io::handle::HandleOrNull")
public class HandleOrNull {
    public static unsafe Self from_raw_handle(RawHandle handle);
}

/** A [hash map] implemented with quadratic probing and SIMD lookup. */
@rust("std::collections::HashMap")
@RustIndexRef
public class HashMap<K, V, S, A> {
    public HashMap();
    public static Map<K, V> with_capacity(uint capacity);
    public static Self new_in(A alloc);
    public static Self with_capacity_in(uint capacity, A alloc);
    public static Map<K, V> with_hasher(S hash_builder);
    public static Map<K, V> with_capacity_and_hasher(uint capacity, S hasher);
    public static Self with_hasher_in(S hash_builder, A alloc);
    public static Self with_capacity_and_hasher_in(uint capacity, S hash_builder, A alloc);
    public uint capacity();
    public Keys<K, V> keys();
    public IntoKeys<K, V, A> into_keys();
    public Values<K, V> values();
    @MutSelf public ValuesMut<K, V> values_mut();
    public IntoValues<K, V, A> into_values();
    public Iter<K, V> iter();
    @MutSelf public IterMut<K, V> iter_mut();
    public uint len();
    public bool is_empty();
    @MutSelf public Drain<K, V, A> drain();
    @MutSelf public ExtractIf<K, V, F, A> extract_if<F>((K, V) -> bool pred);
    @MutSelf public void retain<F>((K, V) -> bool f);
    @MutSelf public void clear();
    public S hasher();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    @MutSelf public Entry<K, V, A> entry(K key);
    public V? get<Q>(&Q k);
    public Tuple<K, V>? get_key_value<Q>(&Q k);
    @MutSelf public V?[] get_disjoint_mut<Q>(Q[] ks);
    @MutSelf public unsafe V?[] get_disjoint_unchecked_mut<Q>(Q[] ks);
    public bool contains_key<Q>(&Q k);
    @MutSelf public V? get_mut<Q>(&Q k);
    @MutSelf public V? insert(K k, V v);
    @MutSelf public V try_insert(K key, V value) throws OccupiedError<K, V, A>;
    @MutSelf public V? remove<Q>(&Q k);
    @MutSelf public Tuple<K, V>? remove_entry<Q>(&Q k);
}

/** A [hash set] implemented as a `HashMap` where the value is `()`. */
@rust("std::collections::HashSet")
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
    public Iter<T> iter();
    public uint len();
    public bool is_empty();
    @MutSelf public Drain<T, A> drain();
    @MutSelf public ExtractIf<T, F, A> extract_if<F>((T) -> bool pred);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void clear();
    public S hasher();
    @MutSelf public void reserve(uint additional);
    @MutSelf public void try_reserve(uint additional) throws TryReserveError;
    @MutSelf public void shrink_to_fit();
    @MutSelf public void shrink_to(uint min_capacity);
    public Difference<T, S, A> difference(&Set<T> other);
    public SymmetricDifference<T, S, A> symmetric_difference(&Set<T> other);
    public Intersection<T, S, A> intersection(&Set<T> other);
    public Union<T, S, A> union(&Set<T> other);
    public bool contains<Q>(&Q value);
    public T? get<Q>(&Q value);
    @MutSelf public T get_or_insert(T value);
    @MutSelf public T get_or_insert_with<Q, F>(&Q value, (Q) -> T f);
    @MutSelf public Entry<T, S, A> entry(T value);
    public bool is_disjoint(&Set<T> other);
    public bool is_subset(&Set<T> other);
    public bool is_superset(&Set<T> other);
    @MutSelf public bool insert(T value);
    @MutSelf public T? replace(T value);
    @MutSelf public bool remove<Q>(&Q value);
    @MutSelf public T? take<Q>(&Q value);
}

/** An iterator that infinitely [`accept`]s connections on a [`TcpListener`]. */
@rust("std::net::tcp::Incoming")
public class Incoming {
}

/** A measurement of a monotonically nondecreasing clock. */
@rust("std::time::Instant")
public class Instant {
    public static Instant now();
    public Duration duration_since(Instant earlier);
    public Duration? checked_duration_since(Instant earlier);
    public Duration saturating_duration_since(Instant earlier);
    public Duration elapsed();
    public Instant? checked_add(Duration duration);
    public Instant? checked_sub(Duration duration);
}

/** A lazy iterator producing elements in the intersection of `BTreeSet`s. */
@rust("std::collections::Intersection")
public class Intersection<T, A> {
}

/** An iterator over the [`char`]s of a string. */
@rust("std::string::IntoChars")
public class IntoChars {
    public String as_str();
    public String into_string();
}

/** An iterator that infinitely [`accept`]s connections on a [`TcpListener`]. */
@rust("std::net::tcp::IntoIncoming")
public class IntoIncoming {
}

/** An error returned by [`BufWriter::into_inner`] which combines an error that */
@rust("std::io::buffered::IntoInnerError")
public class IntoInnerError<W> {
    public Error error();
    public W into_inner();
    public Error into_error();
    public Tuple<Error, W> into_parts();
}

/** An owning iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::IntoIter")
public class IntoIter<T, A> {
    public A allocator();
}

@rust("std::collections::IntoIterSorted")
public class IntoIterSorted<T, A> {
    public A allocator();
}

/** An owning iterator over the keys of a `BTreeMap`. */
@rust("std::collections::IntoKeys")
public class IntoKeys<K, V, A> {
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::fd::raw::IntoRawFd")
public interface IntoRawFd {
    public RawFd into_raw_fd();
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::windows::io::raw::IntoRawHandle")
public interface IntoRawHandle {
    public RawHandle into_raw_handle();
}

/** A trait to express the ability to consume an object and acquire ownership of */
@rust("std::os::windows::io::raw::IntoRawSocket")
public interface IntoRawSocket {
    public RawSocket into_raw_socket();
}

/** An error indicating invalid UTF-8 when converting a [`CString`] into a [`String`]. */
@rust("std::ffi::c_str::IntoStringError")
public class IntoStringError {
    public CString into_cstring();
    public Utf8Error utf8_error();
}

/** An owning iterator over the values of a `BTreeMap`. */
@rust("std::collections::IntoValues")
public class IntoValues<K, V, A> {
}

/** This is the error type used by [`HandleOrInvalid`] when attempting to */
@rust("std::os::windows::io::handle::InvalidHandleError")
public class InvalidHandleError {
}

/** Trait to determine if a descriptor/handle refers to a terminal/tty. */
@rust("std::io::stdio::IsTerminal")
public interface IsTerminal {
    public bool is_terminal();
}

/** An iterator over the elements of a `BinaryHeap`. */
@rust("std::collections::Iter")
public class Iter<T> {
}

/** A mutable iterator over the entries of a `BTreeMap`. */
@rust("std::collections::IterMut")
public class IterMut<K, V> {
}

/** Helper trait for [`[T]::join`](slice::join) */
@rust("std::slice::Join")
public interface Join<Separator> {
    public Output join(&Self slice, Separator sep);
}

/** An owned permission to join on a thread (block on its termination). */
@rust("std::thread::join_handle::JoinHandle")
public class JoinHandle<T> {
    public Thread thread();
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
@rust("std::collections::Keys")
public class Keys<K, V> {
}

/** A value which is initialized on the first access. */
@rust("std::sync::lazy_lock::LazyLock")
public class LazyLock<T, F> {
    public LazyLock(F f);
    public static T into_inner(Self this) throws F;
    public static T force_mut(&LazyLock<T, F> this);
    public static T force(&LazyLock<T, F> this);
    public static T? get_mut(&LazyLock<T, F> this);
    public static T? get(&LazyLock<T, F> this);
}

/** Wraps a writer and buffers output to it, flushing whenever a newline */
@rust("std::io::buffered::linewriter::LineWriter")
public class LineWriter<W> {
    public LineWriter(W inner);
    public static LineWriter<W> with_capacity(uint capacity, W inner);
    @MutSelf public W get_mut();
    public W into_inner() throws IntoInnerError<LineWriter<W>>;
    public W get_ref();
}

/** An iterator over the lines of an instance of `BufRead`. */
@rust("std::io::Lines")
public class Lines<B> {
}

/** A doubly-linked list with owned nodes. */
@rust("std::collections::LinkedList")
public class LinkedList<T, A> {
    public LinkedList();
    @MutSelf public void append(&Self other);
    public static Self new_in(A alloc);
    public Iter<T> iter();
    @MutSelf public IterMut<T> iter_mut();
    public Cursor<T, A> cursor_front();
    @MutSelf public CursorMut<T, A> cursor_front_mut();
    public Cursor<T, A> cursor_back();
    @MutSelf public CursorMut<T, A> cursor_back_mut();
    public bool is_empty();
    public uint len();
    @MutSelf public void clear();
    public bool contains(&T x);
    public T? front();
    @MutSelf public T? front_mut();
    public T? back();
    @MutSelf public T? back_mut();
    @MutSelf public void push_front(T elt);
    @MutSelf public T push_front_mut(T elt);
    @MutSelf public T? pop_front();
    @MutSelf public void push_back(T elt);
    @MutSelf public T push_back_mut(T elt);
    @MutSelf public T? pop_back();
    @MutSelf public LinkedList<T, A> split_off(uint at);
    @MutSelf public T remove(uint at);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public ExtractIf<T, F, A> extract_if<F>((T) -> bool filter);
}

/** A thread local storage (TLS) key which owns its contents. */
@rust("std::thread::local::LocalKey")
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

public const char MAIN_SEPARATOR;

public const String MAIN_SEPARATOR_STR;

/** An RAII mutex guard returned by `MutexGuard::map`, which can point to a */
@rust("std::sync::nonpoison::mutex::MappedMutexGuard")
public class MappedMutexGuard<T> {
    public static MappedMutexGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedMutexGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

/** RAII structure used to release the shared read access of a lock when */
@rust("std::sync::nonpoison::rwlock::MappedRwLockReadGuard")
public class MappedRwLockReadGuard<T> {
    public static MappedRwLockReadGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedRwLockReadGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

/** RAII structure used to release the exclusive write access of a lock when */
@rust("std::sync::nonpoison::rwlock::MappedRwLockWriteGuard")
public class MappedRwLockWriteGuard<T> {
    public static MappedRwLockWriteGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedRwLockWriteGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

/** This struct is used to iterate through the control messages. */
@rust("std::os::unix::net::ancillary::Messages")
public class Messages {
}

/** Metadata information about a file. */
@rust("std::fs::Metadata")
public class Metadata {
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

/** Windows-specific extensions to [`fs::Metadata`]. */
@rust("std::collections::vec_deque::IterMut")
public interface MetadataExt {
    public u32 file_attributes();
    public ulong creation_time();
    public ulong last_access_time();
    public ulong last_write_time();
    public ulong file_size();
    public u32? volume_serial_number();
    public u32? number_of_links();
    public ulong? file_index();
    public ulong? change_time();
}

/** A mutual exclusion primitive useful for protecting shared data that does not keep track of */
@rust("std::sync::nonpoison::mutex::Mutex")
public class Mutex<T> {
    public Mutex(T t);
    public T get_cloned();
    public void set(T value);
    public T replace(T value);
    public MutexGuard<T> lock();
    public TryLockResult<MutexGuard<T>> try_lock();
    public T into_inner();
    @MutSelf public T get_mut();
    public T* data_ptr();
    public R with_mut<F, R>((T) -> R f);
}

/** An RAII implementation of a "scoped lock" of a mutex. When this structure is */
@rust("std::sync::nonpoison::mutex::MutexGuard")
public class MutexGuard<T> {
    public static MappedMutexGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedMutexGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

/** An error returned from [`Path::normalize_lexically`] if a `..` parent reference */
@rust("std::path::NormalizeError")
public class NormalizeError {
}

/** An error indicating that an interior nul byte was found. */
@rust("std::ffi::c_str::NulError")
public class NulError {
    public uint nul_position();
    public List<ubyte> into_vec();
}

/** This is the error type used by [`HandleOrNull`] when attempting to convert */
@rust("std::os::windows::io::handle::NullHandleError")
public class NullHandleError {
}

public const Once ONCE_INIT;

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
@rust("std::collections::OccupiedEntry")
public class OccupiedEntry<K, V, A> {
    public K key();
    public Tuple<K, V> remove_entry();
    public V get();
    @MutSelf public V get_mut();
    public V into_mut();
    @MutSelf public V insert(V value);
    public V remove();
}

/** The error returned by [`try_insert`](BTreeMap::try_insert) when the key already exists. */
@rust("std::collections::OccupiedError")
public struct OccupiedError<K, V, A> {
    public OccupiedEntry<K, V, A> entry;
    public K key;
    public V value;
}

/** A low-level synchronization primitive for one-time global execution. */
@rust("std::sync::once::Once")
public class Once {
    public Once();
    public void call_once<F>(() -> void f);
    public void call_once_force<F>((OnceState) -> void f);
    public bool is_completed();
    public void wait();
    public void wait_force();
}

/** A synchronization primitive which can nominally be written to only once. */
@rust("std::sync::once_lock::OnceLock")
public class OnceLock<T> {
    public OnceLock();
    public T? get();
    @MutSelf public T? get_mut();
    public T wait();
    public void set(T value) throws T;
    public T try_insert(T value) throws Tuple<T, T>;
    public T get_or_init<F>(() -> T f);
    @MutSelf public T get_mut_or_init<F>(() -> T f);
    public T get_or_try_init<F, E>(() -> Result<T, E> f) throws E;
    @MutSelf public T get_mut_or_try_init<F, E>(() -> Result<T, E> f) throws E;
    public T? into_inner();
    @MutSelf public T? take();
}

/** State yielded to [`Once::call_once_force()`]’s closure parameter. The state */
@rust("std::sync::once::OnceState")
public class OnceState {
    public bool is_poisoned();
}

/** Options and flags which can be used to configure how a file is opened. */
@rust("std::fs::OpenOptions")
public class OpenOptions {
    public OpenOptions();
    @MutSelf public Self read(bool read);
    @MutSelf public Self write(bool write);
    @MutSelf public Self append(bool append);
    @MutSelf public Self truncate(bool truncate);
    @MutSelf public Self create(bool create);
    @MutSelf public Self create_new(bool create_new);
    public File open<P>(P path) throws Error;
}

/** Unix-specific extensions to [`fs::OpenOptions`]. */
@rust("std::os::unix::fs::OpenOptionsExt")
public interface OpenOptionsExt {
    @MutSelf public Self mode(u32 mode);
    @MutSelf public Self custom_flags(i32 flags);
}

@rust("std::os::windows::fs::OpenOptionsExt2")
public interface OpenOptionsExt2 {
    @MutSelf public Self freeze_last_access_time(bool freeze);
    @MutSelf public Self freeze_last_write_time(bool freeze);
}

/** Borrowed reference to an OS string (see [`OsString`]). */
@rust("std::ffi::os_str::OsStr")
public class OsStr {
    public OsStr(&S s);
    public static unsafe Self from_encoded_bytes_unchecked(ubyte[] bytes);
    public String? to_str();
    public Cow<String> to_string_lossy();
    public OsString to_os_string();
    public bool is_empty();
    public uint len();
    public OsString into_os_string();
    public Tuple<OsStr, OsStr> split_at(uint mid);
    public Tuple<OsStr, OsStr>? split_at_checked(uint mid);
    public ubyte[] as_encoded_bytes();
    public Self slice_encoded_bytes<R>(R range);
    @MutSelf public void make_ascii_lowercase();
    @MutSelf public void make_ascii_uppercase();
    public OsString to_ascii_lowercase();
    public OsString to_ascii_uppercase();
    public bool is_ascii();
    public bool eq_ignore_ascii_case<S>(S other);
    public Display display();
    public OsStr as_os_str();
}

/** Windows-specific extensions to [`OsStr`]. */
@rust("std::os::windows::ffi::OsStrExt")
public interface OsStrExt {
    public EncodeWide encode_wide();
}

/** A type that can represent owned, mutable platform-native strings, but is */
@rust("std::ffi::os_str::OsString")
public class OsString {
    public OsString();
    public static unsafe Self from_encoded_bytes_unchecked(List<ubyte> bytes);
    public OsStr as_os_str();
    public List<ubyte> into_encoded_bytes();
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
    public OsStr leak();
    @MutSelf public void truncate(uint len);
}

/** Windows-specific extensions to [`OsString`]. */
@rust("std::os::windows::ffi::OsStringExt")
public interface OsStringExt {
    public Self from_wide(ushort[] wide);
}

/** The output of a finished process. */
@rust("std::process::Output")
public class Output {
    public ExitStatus status;
    public List<ubyte> stdout;
    public List<ubyte> stderr;
    public Self exit_ok() throws ExitStatusError;
}

/** An owned file descriptor. */
@rust("std::os::fd::owned::OwnedFd")
public class OwnedFd {
    public Self try_clone() throws Error;
}

/** An owned handle. */
@rust("std::os::windows::io::handle::OwnedHandle")
public class OwnedHandle {
    public Self try_clone() throws Error;
}

/** An owned socket. */
@rust("std::os::windows::io::socket::OwnedSocket")
public class OwnedSocket {
    public Self try_clone() throws Error;
}

/** A struct providing information about a panic. */
@rust("std::panic::PanicHookInfo")
public class PanicHookInfo {
    public Any payload();
    public String? payload_as_str();
    public Location? location();
    public bool can_unwind();
}

/** A slice of a path (akin to [`str`]). */
@rust("std::path::Path")
public class Path {
    public Path(&S s);
    public OsStr as_os_str();
    @MutSelf public OsStr as_mut_os_str();
    public String? to_str();
    public Cow<String> to_string_lossy();
    public PathBuf to_path_buf();
    public bool is_absolute();
    public bool is_relative();
    public bool has_root();
    public Path? parent();
    public Ancestors ancestors();
    public OsStr? file_name();
    public Path strip_prefix<P>(P base) throws StripPrefixError;
    public Path trim_prefix<P>(P base);
    public bool starts_with<P>(P base);
    public bool ends_with<P>(P child);
    public bool is_empty();
    public OsStr? file_stem();
    public OsStr? file_prefix();
    public OsStr? extension();
    public bool has_trailing_sep();
    public Cow<Path> with_trailing_sep();
    public Path trim_trailing_sep();
    public PathBuf join<P>(P path);
    public PathBuf with_file_name<S>(S file_name);
    public PathBuf with_extension<S>(S extension);
    public PathBuf with_added_extension<S>(S extension);
    public Components components();
    public Iter iter();
    public Display display();
    public Path as_path();
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
public class PathBuf {
    public PathBuf();
    public static PathBuf with_capacity(uint capacity);
    public Path as_path();
    public Path leak();
    @MutSelf public void push<P>(P path);
    @MutSelf public bool pop();
    @MutSelf public void set_trailing_sep(bool trailing_sep);
    @MutSelf public void push_trailing_sep();
    @MutSelf public void pop_trailing_sep();
    @MutSelf public void set_file_name<S>(S file_name);
    @MutSelf public bool set_extension<S>(S extension);
    @MutSelf public bool add_extension<S>(S extension);
    @MutSelf public OsString as_mut_os_string();
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
}

/** Structure wrapping a mutable reference to the greatest item on a */
@rust("std::collections::PeekMut")
public class PeekMut<T, A> {
    @MutSelf public bool refresh();
    public static T pop(PeekMut<T, A> this);
}

/** Representation of the various permissions on a file. */
@rust("std::fs::Permissions")
public class Permissions {
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
public class PidFd {
    public void kill() throws Error;
    public ExitStatus wait() throws Error;
    public ExitStatus? try_wait() throws Error;
}

/** Read end of an anonymous pipe. */
@rust("std::io::pipe::PipeReader")
public class PipeReader {
    public Self try_clone() throws Error;
}

/** Write end of an anonymous pipe. */
@rust("std::io::pipe::PipeWriter")
public class PipeWriter {
    public Self try_clone() throws Error;
}

/** A type of error which can be returned whenever a lock is acquired. */
@rust("std::sync::poison::PoisonError")
public class PoisonError<T> {
    public PoisonError(T data);
    public T into_inner();
    public T get_ref();
    @MutSelf public T get_mut();
}

/** Windows path prefixes, e.g., `C:` or `\\server\share`. */
@rust("std::path::Prefix")
public enum Prefix {
    Verbatim(OsStr), VerbatimUNC(OsStr, OsStr), VerbatimDisk(ubyte), DeviceNS(OsStr), UNC(OsStr, OsStr), Disk(ubyte)
}

/** A structure wrapping a Windows path prefix as well as its unparsed string */
@rust("std::path::PrefixComponent")
public class PrefixComponent {
    public Prefix kind();
    public OsStr as_os_str();
}

/** A wrapper around windows [`ProcThreadAttributeList`][1]. */
@rust("std::os::windows::process::ProcThreadAttributeList")
public class ProcThreadAttributeList {
    public static ProcThreadAttributeListBuilder build();
}

/** Builder for constructing a [`ProcThreadAttributeList`]. */
@rust("std::os::windows::process::ProcThreadAttributeListBuilder")
public class ProcThreadAttributeListBuilder {
    public Self attribute<T>(uint attribute, &T value);
    public unsafe Self raw_attribute<T>(uint attribute, T* value_ptr, uint value_size);
    public ProcThreadAttributeList finish() throws Error;
}

/** `RandomState` is the default state for [`HashMap`] types. */
@rust("std::hash::random::RandomState")
public class RandomState {
    public RandomState();
}

/** An iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::Range")
public class Range<K, V> {
}

/** A mutable iterator over a sub-range of entries in a `BTreeMap`. */
@rust("std::collections::RangeMut")
public class RangeMut<K, V> {
}

/** A single-threaded reference-counting pointer. 'Rc' stands for 'Reference */
@rust("std::rc::Rc")
public class Rc<T, A> {
    public Rc(T value);
    public static T new_cyclic<F>((Weak<T>) -> T data_fn);
    public static MaybeUninit<T> new_uninit();
    public static MaybeUninit<T> new_zeroed();
    public static T try_new(T value) throws AllocError;
    public static MaybeUninit<T> try_new_uninit() throws AllocError;
    public static MaybeUninit<T> try_new_zeroed() throws AllocError;
    public static Pin<T> pin(T value);
    public static U map<U>(Self this, (T) -> U f);
    public static TryType try_map<R>(Self this, (T) -> R f);
    public static T new_in(T value, A alloc);
    public static MaybeUninit<T> new_uninit_in(A alloc);
    public static MaybeUninit<T> new_zeroed_in(A alloc);
    public static T new_cyclic_in<F>((Weak<T, A>) -> T data_fn, A alloc);
    public static Self try_new_in(T value, A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_uninit_in(A alloc) throws AllocError;
    public static MaybeUninit<T> try_new_zeroed_in(A alloc) throws AllocError;
    public static Pin<Self> pin_in(T value, A alloc);
    public static T try_unwrap(Self this) throws Self;
    public static T? into_inner(Self this);
    public static MaybeUninit<T>[] new_uninit_slice(uint len);
    public static MaybeUninit<T>[] new_zeroed_slice(uint len);
    public static MaybeUninit<T>[] new_uninit_slice_in(uint len, A alloc);
    public static MaybeUninit<T>[] new_zeroed_slice_in(uint len, A alloc);
    public T[] into_array() throws Self;
    public unsafe T assume_init();
    public static T clone_from_ref(&T value);
    public static T try_clone_from_ref(&T value) throws AllocError;
    public static T clone_from_ref_in(&T value, A alloc);
    public static T try_clone_from_ref_in(&T value, A alloc) throws AllocError;
    public static unsafe Self from_raw(T* ptr);
    public static T* into_raw(Self this);
    public static unsafe void increment_strong_count(T* ptr);
    public static unsafe void decrement_strong_count(T* ptr);
    public static A allocator(&Self this);
    public static Tuple<T*, A> into_raw_with_allocator(Self this);
    public static T* as_ptr(&Self this);
    public static unsafe Self from_raw_in(T* ptr, A alloc);
    public static Weak<T, A> downgrade(&Self this);
    public static uint weak_count(&Self this);
    public static uint strong_count(&Self this);
    public static unsafe void increment_strong_count_in(T* ptr, A alloc);
    public static unsafe void decrement_strong_count_in(T* ptr, A alloc);
    public static T? get_mut(&Self this);
    public static unsafe T get_mut_unchecked(&Self this);
    public static bool ptr_eq(&Self this, &Self other);
    public static T make_mut(&Self this);
    public static T unwrap_or_clone(Self this);
    public T downcast<T>() throws Self;
    public unsafe T downcast_unchecked<T>();
}

/** The `Read` trait allows for reading bytes from a source. */
@rust("std::io::Read")
public interface Read {
    @MutSelf public uint read(ubyte[] buf) throws Error;
    @MutSelf public uint read_vectored(IoSliceMut[] bufs) throws Error;
    public bool is_read_vectored();
    @MutSelf public uint read_to_end(&List<ubyte> buf) throws Error;
    @MutSelf public uint read_to_string(&String buf) throws Error;
    @MutSelf public void read_exact(ubyte[] buf) throws Error;
    @MutSelf public void read_buf(BorrowedCursor<ubyte> buf) throws Error;
    @MutSelf public void read_buf_exact(BorrowedCursor<ubyte> cursor) throws Error;
    @MutSelf public Self by_ref();
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
}

/** The receiving half of Rust's [`channel`] (or [`sync_channel`]) type. */
@rust("std::sync::mpmc::Receiver")
public class Receiver<T> {
    public T try_recv() throws TryRecvError;
    public T recv() throws RecvError;
    public T recv_timeout(Duration timeout) throws RecvTimeoutError;
    public T recv_deadline(Instant deadline) throws RecvTimeoutError;
    public TryIter<T> try_iter();
    public bool is_empty();
    public bool is_full();
    public uint len();
    public uint? capacity();
    public bool same_channel(&Receiver<T> other);
    public Iter<T> iter();
    public bool is_disconnected();
}

/** An error returned from the [`recv`] function on a [`Receiver`]. */
@rust("std::sync::mpsc::RecvError")
public class RecvError {
}

/** This enumeration is the list of possible errors that made [`recv_timeout`] */
@rust("std::sync::mpsc::RecvTimeoutError")
public enum RecvTimeoutError {
    Timeout, Disconnected
}

/** A re-entrant mutual exclusion lock */
@rust("std::sync::reentrant_lock::ReentrantLock")
public class ReentrantLock<T> {
    public ReentrantLock(T t);
    public T into_inner();
    public ReentrantLockGuard<T> lock();
    @MutSelf public T get_mut();
    public T* data_ptr();
}

/** An RAII implementation of a "scoped lock" of a re-entrant lock. When this */
@rust("std::sync::reentrant_lock::ReentrantLockGuard")
public class ReentrantLockGuard<T> {
}

/** An error reporter that prints an error and its sources. */
@rust("std::error::Report")
public class Report<E> {
    public Report(E error);
    public Self pretty(bool pretty);
    public Self show_backtrace(bool show_backtrace);
}

/** A reader-writer lock that does not keep track of lock poisoning. */
@rust("std::sync::nonpoison::rwlock::RwLock")
public class RwLock<T> {
    public RwLock(T t);
    public T get_cloned();
    public void set(T value);
    public T replace(T value);
    public RwLockReadGuard<T> read();
    public TryLockResult<RwLockReadGuard<T>> try_read();
    public RwLockWriteGuard<T> write();
    public TryLockResult<RwLockWriteGuard<T>> try_write();
    public T into_inner();
    @MutSelf public T get_mut();
    public T* data_ptr();
    public R with<F, R>((T) -> R f);
    public R with_mut<F, R>((T) -> R f);
}

/** RAII structure used to release the shared read access of a lock when */
@rust("std::sync::nonpoison::rwlock::RwLockReadGuard")
public class RwLockReadGuard<T> {
    public static MappedRwLockReadGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedRwLockReadGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

/** RAII structure used to release the exclusive write access of a lock when */
@rust("std::sync::nonpoison::rwlock::RwLockWriteGuard")
public class RwLockWriteGuard<T> {
    public static RwLockReadGuard<T> downgrade(Self s);
    public static MappedRwLockWriteGuard<U> map<U, F>(Self orig, (T) -> U f);
    public static MappedRwLockWriteGuard<U> filter_map<U, F>(Self orig, (T) -> U? f) throws Self;
}

public const char[] SEPARATORS;

public const String[] SEPARATORS_STR;

@rust("std::sys::pal::windows::c::windows_sys::SOCKADDR")
public struct SOCKADDR {
    public ushort sa_family;
    public byte[14] sa_data;
}

public const BorrowedFd STDERR;

public const BorrowedFd STDIN;

public const BorrowedFd STDOUT;

@rust("std::os::unix::net::ancillary::ScmCredentials")
public class ScmCredentials {
}

/** This control message contains file descriptors. */
@rust("std::os::unix::net::ancillary::ScmRights")
public class ScmRights {
}

/** A scope to spawn scoped threads in. */
@rust("std::thread::scoped::Scope")
public class Scope {
    public ScopedJoinHandle<T> spawn<F, T>(() -> T f);
}

/** An owned permission to join on a scoped thread (block on its termination). */
@rust("std::thread::scoped::ScopedJoinHandle")
public class ScopedJoinHandle<T> {
    public Thread thread();
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
public enum SeekFrom {
    Start(ulong), End(long), Current(long)
}

/** An error returned from the [`Sender::send`] or [`SyncSender::send`] */
@rust("std::sync::mpsc::SendError")
public class SendError<T> {
}

/** An error returned from the [`send_timeout`] method. */
@rust("std::sync::mpmc::error::SendTimeoutError")
public enum SendTimeoutError<T> {
    Timeout(T), Disconnected(T)
}

/** The sending-half of Rust's synchronous [`channel`] type. */
@rust("std::sync::mpmc::Sender")
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
public enum Shutdown {
    Read, Write, Both
}

@rust("std::sys::net::connection::socket::windows::Socket")
public class Socket {
}

/** An address associated with a Unix socket. */
@rust("std::os::unix::net::addr::SocketAddr")
public class SocketAddr {
    public static SocketAddr from_pathname<P>(P path) throws Error;
    public bool is_unnamed();
    public Path? as_pathname();
}

/** Platform-specific extensions to [`SocketAddr`]. */
@rust("std::os::net::linux_ext::addr::SocketAddrExt")
public interface SocketAddrExt {
    public SocketAddr from_abstract_name<N>(N name) throws Error;
    public ubyte[]? as_abstract_name();
}

/** A Unix socket Ancillary data struct. */
@rust("std::os::unix::net::ancillary::SocketAncillary")
public class SocketAncillary {
    public SocketAncillary(ubyte[] buffer);
    public uint capacity();
    public bool is_empty();
    public uint len();
    public Messages messages();
    public bool truncated();
    @MutSelf public bool add_fds(RawFd[] fds);
    @MutSelf public bool add_creds(SocketCred[] creds);
    @MutSelf public void clear();
}

@rust("std::os::unix::net::ancillary::SocketCred")
public class SocketCred {
}

/** A splicing iterator for `Vec`. */
@rust("std::vec::Splice")
public class Splice<I, A> {
}

/** An iterator over the contents of an instance of `BufRead` split on a */
@rust("std::io::Split")
public class Split<B> {
}

/** An iterator that splits an environment variable into paths according to */
@rust("std::env::SplitPaths")
public class SplitPaths {
}

/** This trait provides a possibly-temporary implementation of float functions */
@rust("std::std_float::StdFloat")
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
@rust("std::io::stdio::Stderr")
public class Stderr {
    public StderrLock lock();
}

/** A locked reference to the [`Stderr`] handle. */
@rust("std::io::stdio::StderrLock")
public class StderrLock {
}

/** A handle to the standard input stream of a process. */
@rust("std::io::stdio::Stdin")
public class Stdin {
    public StdinLock lock();
    public uint read_line(&String buf) throws Error;
    public Lines<StdinLock> lines();
}

/** A locked reference to the [`Stdin`] handle. */
@rust("std::io::stdio::StdinLock")
public class StdinLock {
}

/** Describes what to do with a standard I/O stream for a child process when */
@rust("std::process::Stdio")
public class Stdio {
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
@rust("std::io::stdio::Stdout")
public class Stdout {
    public StdoutLock lock();
}

/** A locked reference to the [`Stdout`] handle. */
@rust("std::io::stdio::StdoutLock")
public class StdoutLock {
}

/** A UTF-8–encoded, growable string. */
@rust("std::string::String")
public class String {
    public String();
    public static String with_capacity(uint capacity);
    public static String try_with_capacity(uint capacity) throws TryReserveError;
    public static String from_utf8(List<ubyte> vec) throws FromUtf8Error;
    public static Cow<String> from_utf8_lossy(ubyte[] v);
    public static String from_utf8_lossy_owned(List<ubyte> v);
    public static String from_utf16(ushort[] v) throws FromUtf16Error;
    public static String from_utf16_lossy(ushort[] v);
    public static String from_utf16le(ubyte[] v) throws FromUtf16Error;
    public static String from_utf16le_lossy(ubyte[] v);
    public static String from_utf16be(ubyte[] v) throws FromUtf16Error;
    public static String from_utf16be_lossy(ubyte[] v);
    public Tuple<ubyte*, uint, uint> into_raw_parts();
    public static unsafe String from_raw_parts(ubyte* buf, uint length, uint capacity);
    public static unsafe String from_utf8_unchecked(List<ubyte> bytes);
    public List<ubyte> into_bytes();
    public String as_str();
    @MutSelf public String as_mut_str();
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
    public ubyte[] as_bytes();
    @MutSelf public void truncate(uint new_len);
    @MutSelf public char? pop();
    @MutSelf public char remove(uint idx);
    @MutSelf public void remove_matches<P>(P pat);
    @MutSelf public void retain<F>((char) -> bool f);
    @MutSelf public void insert(uint idx, char ch);
    @MutSelf public void insert_str(uint idx, &String string);
    @MutSelf public unsafe List<ubyte> as_mut_vec();
    public uint len();
    public bool is_empty();
    @MutSelf public String split_off(uint at);
    @MutSelf public void clear();
    @MutSelf public Drain drain<R>(R range);
    public IntoChars into_chars();
    @MutSelf public void replace_range<R>(R range, &String replace_with);
    @MutSelf public void replace_first<P>(P from, &String to);
    @MutSelf public void replace_last<P>(P from, &String to);
    public String into_boxed_str();
    public String leak();
}

/** An error returned from [`Path::strip_prefix`] if the prefix was not found. */
@rust("std::path::StripPrefixError")
public class StripPrefixError {
}

/** A lazy iterator producing elements in the symmetric difference of `BTreeSet`s. */
@rust("std::collections::SymmetricDifference")
public class SymmetricDifference<T> {
}

/** The sending-half of Rust's synchronous [`sync_channel`] type. */
@rust("std::sync::mpsc::SyncSender")
public class SyncSender<T> {
    public void send(T t) throws SendError<T>;
    public void try_send(T t) throws TrySendError<T>;
}

/** The default memory allocator provided by the operating system. */
@rust("std::alloc::System")
public class System {
}

/** The system random number generator. */
@rust("std::random::SystemRng")
public class SystemRng {
}

/** A measurement of the system clock, useful for talking to */
@rust("std::time::SystemTime")
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
public class SystemTimeError {
    public Duration duration();
}

/** A TCP socket server, listening for connections. */
@rust("std::net::tcp::TcpListener")
public class TcpListener {
    public static TcpListener bind<A>(A addr) throws Error;
    public SocketAddr local_addr() throws Error;
    public TcpListener try_clone() throws Error;
    public Tuple<TcpStream, SocketAddr> accept() throws Error;
    public Incoming incoming();
    public IntoIncoming into_incoming();
    public void set_ttl(u32 ttl) throws Error;
    public u32 ttl() throws Error;
    public void set_only_v6(bool only_v6) throws Error;
    public bool only_v6() throws Error;
    public Error? take_error() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
}

/** A TCP stream between a local and a remote socket. */
@rust("std::net::tcp::TcpStream")
public class TcpStream {
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
    public uint peek(ubyte[] buf) throws Error;
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
@rust("std::os::net::linux_ext::tcp::TcpStreamExt")
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
public class ThinBox<T> {
    public ThinBox(T value);
    public static Self try_new(T value) throws AllocError;
    public static Self new_unsize<T>(T value);
}

/** A handle to a thread. */
@rust("std::thread::thread::Thread")
public class Thread {
    public void unpark();
    public ThreadId id();
    public String? name();
    public Object* into_raw();
    public static unsafe Thread from_raw(Object* ptr);
}

/** A unique identifier for a running thread. */
@rust("std::thread::id::ThreadId")
public class ThreadId {
    public NonZero<ulong> as_u64();
}

/** A generalization of `Clone` to borrowed data. */
@rust("std::borrow::ToOwned")
public interface ToOwned {
    public Owned to_owned();
    public void clone_into(&Owned target);
}

/** A trait for objects which can be converted or resolved to one or more */
@rust("std::net::socket_addr::ToSocketAddrs")
public interface ToSocketAddrs {
    public Iter to_socket_addrs() throws Error;
}

/** A trait for converting a value to a `String`. */
@rust("std::string::ToString")
public interface ToString {
    public String to_string();
}

/** An iterator that attempts to yield all pending values for a [`Receiver`], */
@rust("std::sync::mpmc::TryIter")
public class TryIter<T> {
}

/** An enumeration of possible errors which can occur while trying to acquire a lock */
@rust("std::fs::TryLockError")
public enum TryLockError {
    Error(Error), WouldBlock
}

/** This enumeration is the list of the possible reasons that [`try_recv`] could */
@rust("std::sync::mpsc::TryRecvError")
public enum TryRecvError {
    Empty, Disconnected
}

/** The error type for `try_reserve` methods. */
@rust("std::collections::TryReserveError")
public class TryReserveError {
    public TryReserveErrorKind kind();
}

/** Details of the allocation that caused a `TryReserveError` */
@rust("std::collections::TryReserveErrorKind")
public enum TryReserveErrorKind {
    CapacityOverflow, AllocError
}

/** This enumeration is the list of the possible error outcomes for the */
@rust("std::sync::mpsc::TrySendError")
public enum TrySendError<T> {
    Full(T), Disconnected(T)
}

public const SystemTime UNIX_EPOCH;

/** A UDP socket. */
@rust("std::net::udp::UdpSocket")
public class UdpSocket {
    public static UdpSocket bind<A>(A addr) throws Error;
    public Tuple<uint, SocketAddr> recv_from(ubyte[] buf) throws Error;
    public Tuple<uint, SocketAddr> peek_from(ubyte[] buf) throws Error;
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
    public uint recv(ubyte[] buf) throws Error;
    public uint peek(ubyte[] buf) throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
}

/** A lazy iterator producing elements in the union of `BTreeSet`s. */
@rust("std::collections::Union")
public class Union<T> {
}

/** A uniquely owned [`Arc`]. */
@rust("std::sync::UniqueArc")
public class UniqueArc<T, A> {
    public UniqueArc(T value);
    public static UniqueArc<U> map<U>(Self this, (T) -> U f);
    public static TryType try_map<R>(Self this, (T) -> R f);
    public static Self new_in(T data, A alloc);
    public static T into_arc(Self this);
    public static Weak<T, A> downgrade(&Self this);
}

/** A uniquely owned [`Rc`]. */
@rust("std::rc::UniqueRc")
public class UniqueRc<T, A> {
    public UniqueRc(T value);
    public static UniqueRc<U> map<U>(Self this, (T) -> U f);
    public static TryType try_map<R>(Self this, (T) -> R f);
    public static Self new_in(T value, A alloc);
    public static T into_rc(Self this);
    public static Weak<T, A> downgrade(&Self this);
}

/** A Unix datagram socket. */
@rust("std::os::unix::net::datagram::UnixDatagram")
public class UnixDatagram {
    public static UnixDatagram bind<P>(P path) throws Error;
    public static UnixDatagram bind_addr(&SocketAddr socket_addr) throws Error;
    public static UnixDatagram unbound() throws Error;
    public static Tuple<UnixDatagram, UnixDatagram> pair() throws Error;
    public void connect<P>(P path) throws Error;
    public void connect_addr(&SocketAddr socket_addr) throws Error;
    public UnixDatagram try_clone() throws Error;
    public SocketAddr local_addr() throws Error;
    public SocketAddr peer_addr() throws Error;
    public Tuple<uint, SocketAddr> recv_from(ubyte[] buf) throws Error;
    public uint recv(ubyte[] buf) throws Error;
    public Tuple<uint, bool, SocketAddr> recv_vectored_with_ancillary_from(IoSliceMut[] bufs, &SocketAncillary ancillary) throws Error;
    public Tuple<uint, bool> recv_vectored_with_ancillary(IoSliceMut[] bufs, &SocketAncillary ancillary) throws Error;
    public uint send_to<P>(ubyte[] buf, P path) throws Error;
    public uint send_to_addr(ubyte[] buf, &SocketAddr socket_addr) throws Error;
    public uint send(ubyte[] buf) throws Error;
    public uint send_vectored_with_ancillary_to<P>(IoSlice[] bufs, &SocketAncillary ancillary, P path) throws Error;
    public uint send_vectored_with_ancillary(IoSlice[] bufs, &SocketAncillary ancillary) throws Error;
    public void set_read_timeout(Duration? timeout) throws Error;
    public void set_write_timeout(Duration? timeout) throws Error;
    public Duration? read_timeout() throws Error;
    public Duration? write_timeout() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
    public void set_mark(u32 mark) throws Error;
    public Error? take_error() throws Error;
    public void shutdown(Shutdown how) throws Error;
    public uint peek(ubyte[] buf) throws Error;
    public Tuple<uint, SocketAddr> peek_from(ubyte[] buf) throws Error;
}

/** A structure representing a Unix domain socket server. */
@rust("std::os::unix::net::listener::UnixListener")
public class UnixListener {
    public static UnixListener bind<P>(P path) throws Error;
    public static UnixListener bind_addr(&SocketAddr socket_addr) throws Error;
    public Tuple<UnixStream, SocketAddr> accept() throws Error;
    public UnixListener try_clone() throws Error;
    public SocketAddr local_addr() throws Error;
    public void set_nonblocking(bool nonblocking) throws Error;
    public Error? take_error() throws Error;
    public Incoming incoming();
}

/** Linux-specific functionality for `AF_UNIX` sockets [`UnixDatagram`] */
@rust("std::os::net::linux_ext::socket::UnixSocketExt")
public interface UnixSocketExt {
    public bool passcred() throws Error;
    public void set_passcred(bool passcred) throws Error;
}

/** A Unix stream socket. */
@rust("std::os::unix::net::stream::UnixStream")
public class UnixStream {
    public static UnixStream connect<P>(P path) throws Error;
    public static UnixStream connect_addr(&SocketAddr socket_addr) throws Error;
    public static Tuple<UnixStream, UnixStream> pair() throws Error;
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
    public uint peek(ubyte[] buf) throws Error;
    public uint recv_vectored_with_ancillary(IoSliceMut[] bufs, &SocketAncillary ancillary) throws Error;
    public uint send_vectored_with_ancillary(IoSlice[] bufs, &SocketAncillary ancillary) throws Error;
}

/** Error type returned by [`CursorMut::insert_before`] and */
@rust("std::collections::UnorderedKeyError")
public class UnorderedKeyError {
}

/** A view into a vacant entry in a `BTreeMap`. */
@rust("std::collections::VacantEntry")
public class VacantEntry<K, V, A> {
    public K key();
    public K into_key();
    public V insert(V value);
    public OccupiedEntry<K, V, A> insert_entry(V value);
}

/** An iterator over the values of a `BTreeMap`. */
@rust("std::collections::Values")
public class Values<K, V> {
}

/** A mutable iterator over the values of a `BTreeMap`. */
@rust("std::collections::ValuesMut")
public class ValuesMut<K, V> {
}

/** The error type for operations interacting with environment variables. */
@rust("std::env::VarError")
public enum VarError {
    NotPresent, NotUnicode(OsString)
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::Vars")
public class Vars {
}

/** An iterator over a snapshot of the environment variables of this process. */
@rust("std::env::VarsOs")
public class VarsOs {
}

/** A contiguous growable array type, written as `Vec<T>`, short for 'vector'. */
@rust("std::vec::Vec")
public class Vec<T, A> {
    public Vec();
    public static Self with_capacity(uint capacity);
    public static Self try_with_capacity(uint capacity) throws TryReserveError;
    public static unsafe Self from_raw_parts(T* ptr, uint length, uint capacity);
    public static unsafe Self from_parts(NonNull<T> ptr, uint length, uint capacity);
    public static Self from_fn<F>(uint length, (uint) -> T f);
    public Tuple<T*, uint, uint> into_raw_parts();
    public Tuple<NonNull<T>, uint, uint> into_parts();
    public T[] const_make_global();
    public static Self with_capacity_in(uint capacity, A alloc);
    @MutSelf public void push(T value);
    @MutSelf public T push_mut(T value);
    public static Self new_in(A alloc);
    public static Self try_with_capacity_in(uint capacity, A alloc) throws TryReserveError;
    public static unsafe Self from_raw_parts_in(T* ptr, uint length, uint capacity, A alloc);
    public static unsafe Self from_parts_in(NonNull<T> ptr, uint length, uint capacity, A alloc);
    public Tuple<T*, uint, uint, A> into_raw_parts_with_alloc();
    public Tuple<NonNull<T>, uint, uint, A> into_parts_with_alloc();
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
    public T[] as_slice();
    @MutSelf public T[] as_mut_slice();
    public T* as_ptr();
    @MutSelf public T* as_mut_ptr();
    @MutSelf public NonNull<T> as_non_null();
    public A allocator();
    @MutSelf public unsafe void set_len(uint new_len);
    @MutSelf public T swap_remove(uint index);
    @MutSelf public void insert(uint index, T element);
    @MutSelf public T insert_mut(uint index, T element);
    @MutSelf public T remove(uint index);
    @MutSelf public T? try_remove(uint index);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void retain_mut<F>((T) -> bool f);
    @MutSelf public void dedup_by_key<F, K>((T) -> K key);
    @MutSelf public void dedup_by<F>((T, T) -> bool same_bucket);
    @MutSelf public T push_within_capacity(T value) throws T;
    @MutSelf public T? pop();
    @MutSelf public T? pop_if((T) -> bool predicate);
    @MutSelf public PeekMut<T, A>? peek_mut();
    @MutSelf public void append(&Self other);
    @MutSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public void clear();
    public uint len();
    public bool is_empty();
    @MutSelf public Self split_off(uint at);
    @MutSelf public void resize_with<F>(uint new_len, () -> T f);
    public T[] leak();
    @MutSelf public MaybeUninit<T>[] spare_capacity_mut();
    @MutSelf public Tuple<T[], MaybeUninit<T>[]> split_at_spare_mut();
    public List<T[]> into_chunks();
    public List<U> recycle<U>();
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_slice(T[] other);
    @MutSelf public void extend_from_within<R>(R src);
    public List<T> into_flattened();
    @MutSelf public void dedup();
    @MutSelf public Splice<IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
}

/** A double-ended queue implemented with a growable ring buffer. */
@rust("std::collections::VecDeque")
public class VecDeque<T, A> {
    public VecDeque();
    @MutSelf public ExtractIf<T, F, A> extract_if<F, R>(R range, (T) -> bool filter);
    public static VecDeque<T> with_capacity(uint capacity);
    public static VecDeque<T> try_with_capacity(uint capacity) throws TryReserveError;
    public static VecDeque<T, A> new_in(A alloc);
    public static VecDeque<T, A> with_capacity_in(uint capacity, A alloc);
    public T? get(uint index);
    @MutSelf public T? get_mut(uint index);
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
    public A allocator();
    public Iter<T> iter();
    @MutSelf public IterMut<T> iter_mut();
    public Tuple<T[], T[]> as_slices();
    @MutSelf public Tuple<T[], T[]> as_mut_slices();
    public uint len();
    public bool is_empty();
    public Iter<T> range<R>(R range);
    @MutSelf public IterMut<T> range_mut<R>(R range);
    @MutSelf public Drain<T, A> drain<R>(R range);
    @MutSelf public Splice<IntoIter, A> splice<R, I>(R range, I replace_with);
    @MutSelf public void clear();
    public bool contains(&T x);
    public T? front();
    @MutSelf public T? front_mut();
    public T? back();
    @MutSelf public T? back_mut();
    @MutSelf public T? pop_front();
    @MutSelf public T? pop_back();
    @MutSelf public T? pop_front_if((T) -> bool predicate);
    @MutSelf public T? pop_back_if((T) -> bool predicate);
    @MutSelf public void push_front(T value);
    @MutSelf public T push_front_mut(T value);
    @MutSelf public void push_back(T value);
    @MutSelf public T push_back_mut(T value);
    @MutSelf public void prepend<I>(I other);
    @MutSelf public void extend_front<I>(I iter);
    @MutSelf public T? swap_remove_front(uint index);
    @MutSelf public T? swap_remove_back(uint index);
    @MutSelf public void insert(uint index, T value);
    @MutSelf public T insert_mut(uint index, T value);
    @MutSelf public T? remove(uint index);
    @MutSelf public Self split_off(uint at);
    @MutSelf public void append(&Self other);
    @MutSelf public void retain<F>((T) -> bool f);
    @MutSelf public void retain_mut<F>((T) -> bool f);
    @MutSelf public void resize_with(uint new_len, () -> T generator);
    @MutSelf public T[] make_contiguous();
    @MutSelf public void rotate_left(uint n);
    @MutSelf public void rotate_right(uint n);
    public uint binary_search(&T x) throws uint;
    public uint binary_search_by<F>((T) -> Ordering f) throws uint;
    public uint binary_search_by_key<B, F>(&B b, (T) -> B f) throws uint;
    public uint partition_point<P>((T) -> bool pred);
    @MutSelf public void resize(uint new_len, T value);
    @MutSelf public void extend_from_within<R>(R src);
    @MutSelf public void prepend_from_within<R>(R src);
}

/** A type indicating whether a timed wait on a condition variable returned */
@rust("std::sync::WaitTimeoutResult")
public class WaitTimeoutResult {
    public bool timed_out();
}

/** The implementation of waking a task on an executor. */
@rust("std::task::Wake")
public interface Wake {
    public void wake();
    public void wake_by_ref();
}

/** `Weak` is a version of [`Rc`] that holds a non-owning reference to the */
@rust("std::rc::Weak")
public class Weak<T, A> {
    public Weak();
    public static Weak<T, A> new_in(A alloc);
    public static unsafe Self from_raw(T* ptr);
    public T* into_raw();
    public A allocator();
    public T* as_ptr();
    public Tuple<T*, A> into_raw_with_allocator();
    public static unsafe Self from_raw_in(T* ptr, A alloc);
    public T? upgrade();
    public uint strong_count();
    public uint weak_count();
    public bool ptr_eq(&Self other);
}

/** A lock could not be acquired at this time because the operation would otherwise block. */
@rust("std::sync::nonpoison::WouldBlock")
public class WouldBlock {
}

/** A trait for objects which are byte-oriented sinks. */
@rust("std::io::Write")
public interface Write {
    @MutSelf public uint write(ubyte[] buf) throws Error;
    @MutSelf public uint write_vectored(IoSlice[] bufs) throws Error;
    public bool is_write_vectored();
    @MutSelf public void flush() throws Error;
    @MutSelf public void write_all(ubyte[] buf) throws Error;
    @MutSelf public void write_all_vectored(IoSlice[] bufs) throws Error;
    @MutSelf public void write_fmt(Arguments args) throws Error;
    @MutSelf public Self by_ref();
}

/** Error returned for the buffered data from `BufWriter::into_parts`, when the underlying */
@rust("std::io::buffered::bufwriter::WriterPanicked")
public class WriterPanicked {
    public List<ubyte> into_inner();
}

/** An iterator that produces directory paths from XDG environment configuration. */
@rust("std::os::unix::xdg::XdgDirsIter")
public class XdgDirsIter {
}

@rust("std::process::abort")
public never abort();

@rust("std::path::absolute")
public PathBuf absolute<P>(P path) throws Error;

@rust("std::thread::spawnhook::add_spawn_hook")
public void add_spawn_hook<F, G>((Thread) -> G hook);

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

@rust("std::thread::functions::available_parallelism")
public NonZero<uint> available_parallelism() throws Error;

@rust("std::panicking::begin_panic")
public never begin_panic<M>(M msg);

@rust("std::os::unix::xdg::cache_home_dir")
public PathBuf cache_home_dir();

@rust("std::fs::canonicalize")
public PathBuf canonicalize<P>(P path) throws Error;

@rust("std::panic::catch_unwind")
public R catch_unwind<F, R>(() -> R f) throws Error;

@rust("std::sync::mpmc::channel")
public Tuple<Sender<T>, Receiver<T>> channel<T>();

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

@rust("std::thread::current::current")
public Thread current();

@rust("std::env::current_dir")
public PathBuf current_dir() throws Error;

@rust("std::env::current_exe")
public PathBuf current_exe() throws Error;

@rust("std::thread::current::current_id")
public ThreadId current_id();

@rust("std::os::unix::xdg::data_dirs")
public XdgDirsIter data_dirs();

@rust("std::os::unix::xdg::data_home_dir")
public PathBuf data_home_dir();

@rust("std::alloc::dealloc")
public unsafe void dealloc(ubyte* ptr, Layout layout);

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

@rust("std::alloc::handle_alloc_error")
public never handle_alloc_error(Layout layout);

@rust("std::fs::hard_link")
public void hard_link<P, Q>(P original, Q link) throws Error;

@rust("std::env::home_dir")
public PathBuf? home_dir();

@rust("std::net::hostname::hostname")
public OsString hostname() throws Error;

@rust("std::process::id")
public u32 id();

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

@rust("std::panic::panic_any")
public never panic_any<M>(M msg);

@rust("std::thread::functions::panicking")
public bool panicking();

@rust("std::os::unix::process::parent_id")
public u32 parent_id();

@rust("std::thread::functions::park")
public void park();

@rust("std::thread::functions::park_timeout")
public void park_timeout(Duration dur);

@rust("std::thread::functions::park_timeout_ms")
public void park_timeout_ms(u32 ms);

@rust("std::io::pipe::pipe")
public Tuple<PipeReader, PipeWriter> pipe() throws Error;

@rust("std::random::random")
public T random<T>(Distribution dist);

@rust("std::fs::read")
public List<ubyte> read<P>(P path) throws Error;

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

@rust("std::panic::resume_unwind")
public never resume_unwind(Any payload);

@rust("std::thread::scoped::scope")
public T scope<F, T>((Scope) -> T f);

@rust("std::alloc::set_alloc_error_hook")
public void set_alloc_error_hook((Layout) -> void hook);

@rust("std::panic::set_backtrace_style")
public void set_backtrace_style(BacktraceStyle style);

@rust("std::env::set_current_dir")
public void set_current_dir<P>(P path) throws Error;

@rust("std::panicking::set_hook")
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

@rust("std::thread::functions::sleep")
public void sleep(Duration dur);

@rust("std::thread::functions::sleep_ms")
public void sleep_ms(u32 ms);

@rust("std::thread::functions::sleep_until")
public void sleep_until(Instant deadline);

@rust("std::fs::soft_link")
public void soft_link<P, Q>(P original, Q link) throws Error;

@rust("std::thread::functions::spawn")
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

@rust("std::io::stdio::stderr")
public Stderr stderr();

@rust("std::io::stdio::stdin")
public Stdin stdin();

@rust("std::io::stdio::stdout")
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
public Tuple<Sender<T>, Receiver<T>> sync_channel<T>(uint cap);

@rust("std::alloc::take_alloc_error_hook")
public (Layout) -> void take_alloc_error_hook();

@rust("std::panicking::take_hook")
public Fn take_hook();

@rust("std::env::temp_dir")
public PathBuf temp_dir();

@rust("std::panicking::update_hook")
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

@rust("std::thread::functions::yield_now")
public void yield_now();
