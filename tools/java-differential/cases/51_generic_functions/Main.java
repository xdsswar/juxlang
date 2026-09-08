public class Main {
    static class Box<T> {
        private T item;
        Box(T item) { this.item = item; }
        T get() { return this.item; }
        String show() { return "Box(" + this.item + ")"; }
        <U> String pair(U other) { return this.item + "|" + other; }
    }

    static <T> String describe(T value) {
        return "value=" + value;
    }

    static <T> T firstOf(java.util.List<T> xs) {
        return xs.get(0);
    }

    public static void main(String[] args) {
        Box<String> b = new Box<>("hi");
        System.out.println(b.get());
        System.out.println(b.show());
        Box<Integer> n = new Box<>(5);
        System.out.println(n.show());
        System.out.println(n.get() + 1);
        System.out.println(describe("text"));
        System.out.println(describe(42));
        System.out.println(b.pair(7));
        System.out.println(n.pair("tail"));
        java.util.List<String> xs = new java.util.ArrayList<>();
        xs.add("one");
        xs.add("two");
        System.out.println(firstOf(xs));
        Box<Box<Integer>> nested = new Box<>(new Box<>(9));
        System.out.println(nested.get().show());
        System.out.println(nested.get().get());
    }
}
