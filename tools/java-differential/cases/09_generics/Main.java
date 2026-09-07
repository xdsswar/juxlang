public class Main {
    static class Pair<A, B> {
        private A first;
        private B second;
        Pair(A a, B b) { this.first = a; this.second = b; }
        A getFirst() { return this.first; }
        B getSecond() { return this.second; }
        String show() { return "(" + this.first + ", " + this.second + ")"; }
    }
    static class Cell<T> {
        private T value;
        Cell(T v) { this.value = v; }
        T get() { return this.value; }
        void set(T v) { this.value = v; }
    }

    public static void main(String[] args) {
        Pair<Integer, String> p = new Pair<>(1, "one");
        System.out.println(p.getFirst());
        System.out.println(p.getSecond());
        System.out.println(p.show());

        Cell<String> c = new Cell<>("start");
        System.out.println(c.get());
        c.set("changed");
        System.out.println(c.get());

        Pair<String, Pair<Integer, Integer>> nested =
            new Pair<>("k", new Pair<>(2, 3));
        System.out.println(nested.getFirst());
        System.out.println(nested.getSecond().show());
    }
}
