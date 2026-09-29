package rust.fam.inner;

import rust.fam.*;

public type Clock = rust.fam.Clock;

@rust("fam::inner::Frame")
@RustClone
@RustDebug
public class Frame {
    public static Frame none();
    public static Frame with_margin(ulong m);
}
