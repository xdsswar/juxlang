public class Main {
    interface Container<T> {
        T get();
        String label();

        default String show() {
            return this.label() + "(" + this.get() + ")";
        }
    }

    static class IntBox implements Container<Integer> {
        private int v;
        IntBox(int v) { this.v = v; }
        public Integer get() { return this.v; }
        public String label() { return "int-box"; }
    }

    static class Wrapper<T> implements Container<T> {
        private T v;
        Wrapper(T v) { this.v = v; }
        public T get() { return this.v; }
        public String label() { return "wrapper"; }
    }

    static <T> String render(Container<T> c) {
        return "[" + c.show() + "]";
    }

    public static void main(String[] args) {
        IntBox a = new IntBox(7);
        System.out.println(a.show());
        System.out.println(a.get() + 1);

        Wrapper<String> b = new Wrapper<>("hi");
        System.out.println(b.show());
        System.out.println(b.get());

        System.out.println(render(a));
        System.out.println(render(b));

        Wrapper<Wrapper<Integer>> c = new Wrapper<>(new Wrapper<>(3));
        System.out.println(c.get().show());
        System.out.println(c.get().get());
    }
}
