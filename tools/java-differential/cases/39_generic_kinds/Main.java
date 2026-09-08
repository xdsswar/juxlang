public class Main {
    record Cell<T>(T value) {
        String show() { return "cell(" + this.value + ")"; }
    }
    record Duo<A, B>(A left, B right) {
        String show() { return "pair(" + this.left + ", " + this.right + ")"; }
    }
    static class Wrap<T> {
        private T v;
        Wrap(T v) { this.v = v; }
        String show() { return "wrap(" + this.v + ")"; }
    }
    static class Emitter {
        <T> String mark(T x) { return "m:" + x; }
    }

    public static void main(String[] args) {
        System.out.println(new Cell<>(5).show());
        System.out.println(new Cell<>("x").show());
        System.out.println(new Duo<>(1, "two").show());
        System.out.println(new Wrap<>("plain").show());
        System.out.println(new Wrap<>(new Cell<>("in")).show());
        Emitter e = new Emitter();
        System.out.println(e.mark("s"));
        System.out.println(e.mark(7));
    }
}
