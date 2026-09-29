// A hand-written stub of a crate family (Bindgen G.6.2.4, ERRATA E132): the
// host `fam`, and a nested package for its member crate `inner`.
package rust.fam;

@rust("fam::Clock")
@RustClone
@RustDebug
public class Clock {
    public static final Clock EPOCH;
    public static Clock from_secs(ulong secs);
    public ulong as_secs();
}

@rust("fam::Mode")
public enum Mode {
    Fast, Slow
}
