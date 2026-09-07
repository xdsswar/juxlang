public class Main {
    static abstract class Shape {
        abstract double area();
        abstract String kind();
        String label() { return this.kind() + "=" + this.area(); }
    }
    static class Sq extends Shape {
        private double s;
        Sq(double s) { this.s = s; }
        double area() { return this.s * this.s; }
        String kind() { return "sq"; }
    }
    static class Tri extends Shape {
        private double b;
        private double h;
        Tri(double b, double h) { this.b = b; this.h = h; }
        double area() { return this.b * this.h / 2.0; }
        String kind() { return "tri"; }
    }
    static class Holder<T extends Shape> {
        private T item;
        Holder(T item) { this.item = item; }
        T get() { return this.item; }
        String report() { return "holds " + this.item.label(); }
        double doubled() { return this.item.area() * 2.0; }
    }

    static <T extends Shape> String biggest(T a, T b) {
        if (a.area() >= b.area()) { return a.kind(); }
        return b.kind();
    }

    public static void main(String[] args) {
        Holder<Sq> hs = new Holder<>(new Sq(3.0));
        System.out.println(hs.report());
        System.out.println(hs.doubled());
        System.out.println(hs.get().kind());

        Holder<Tri> ht = new Holder<>(new Tri(4.0, 6.0));
        System.out.println(ht.report());
        System.out.println(ht.doubled());

        System.out.println(biggest(new Sq(2.0), new Sq(5.0)));

        Holder<Sq> nested = new Holder<>(new Sq(1.5));
        System.out.println(nested.get().area());
    }
}
