public class Main {
    static class Cell<T> {
        private T value;
        Cell(T v) { this.value = v; }
        T get() { return this.value; }
        void set(T v) { this.value = v; }
        String show() { return "[" + this.value + "]"; }
    }

    public static void main(String[] args) {
        Cell<Integer> one = new Cell<>(1);
        System.out.println(one.get());
        System.out.println(one.show());

        Cell<Cell<Integer>> two = new Cell<>(new Cell<>(2));
        System.out.println(two.get().get());
        System.out.println(two.get().show());

        Cell<Cell<Cell<Integer>>> three = new Cell<>(new Cell<>(new Cell<>(3)));
        System.out.println(three.get().get().get());
        System.out.println(three.get().get().show());

        Cell<Integer> inner = three.get().get();
        inner.set(30);
        System.out.println(three.get().get().get());

        two.set(new Cell<>(22));
        System.out.println(two.get().get());
    }
}
