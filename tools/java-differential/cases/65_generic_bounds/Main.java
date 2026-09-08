public class Main {
    static class Shape {
        String name() { return "shape"; }
        int area() { return 0; }
    }

    static class Square extends Shape {
        private int side;
        Square(int side) { this.side = side; }
        String name() { return "square"; }
        int area() { return this.side * this.side; }
    }

    static <T extends Shape> String describe(T s) {
        return s.name() + "=" + s.area();
    }

    static <T extends Shape> int totalOf(java.util.List<T> shapes) {
        int t = 0;
        for (T s : shapes) {
            t = t + s.area();
        }
        return t;
    }

    static class Holder<T extends Shape> {
        private T item;
        Holder(T item) { this.item = item; }
        String render() { return "held " + this.item.name(); }
        T get() { return this.item; }
    }

    public static void main(String[] args) {
        System.out.println(describe(new Square(3)));
        System.out.println(describe(new Shape()));

        java.util.List<Square> squares = new java.util.ArrayList<>();
        squares.add(new Square(2));
        squares.add(new Square(3));
        System.out.println(totalOf(squares));

        Holder<Square> h = new Holder<>(new Square(5));
        System.out.println(h.render());
        System.out.println(h.get().area());
    }
}
