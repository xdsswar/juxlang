public class Main {
    static class Box<T> {
        private T v;
        Box(T v) { this.v = v; }
        T get() { return this.v; }
    }

    static <T> T identity(T x) { return x; }
    static <T> String describe(Box<T> b) { return "box(" + b.get() + ")"; }
    static <A, B> String both(A a, B b) { return a + "/" + b; }
    static <T> Box<T> rewrap(Box<T> b) { return new Box<>(b.get()); }

    public static void main(String[] args) {
        System.out.println(identity(5));
        System.out.println(identity("s"));
        System.out.println(identity(true));

        System.out.println(describe(new Box<>(7)));
        System.out.println(describe(new Box<>("q")));

        System.out.println(both(1, "a"));
        System.out.println(both("x", 2.5));

        System.out.println(rewrap(new Box<>(3)).get());
        System.out.println(rewrap(rewrap(new Box<>("z"))).get());
    }
}
