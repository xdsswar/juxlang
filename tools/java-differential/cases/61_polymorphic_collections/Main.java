public class Main {
    static class Shape {
        String name() { return "shape"; }
        int area() { return 0; }
        String describe() { return this.name() + "=" + this.area(); }
    }

    static class Square extends Shape {
        private int side;
        Square(int side) { this.side = side; }
        String name() { return "square"; }
        int area() { return this.side * this.side; }
    }

    static class Rect extends Shape {
        private int w;
        private int h;
        Rect(int w, int h) { this.w = w; this.h = h; }
        String name() { return "rect"; }
        int area() { return this.w * this.h; }
    }

    static int totalArea(java.util.List<Shape> shapes) {
        int t = 0;
        for (Shape s : shapes) {
            t = t + s.area();
        }
        return t;
    }

    public static void main(String[] args) {
        java.util.List<Shape> shapes = new java.util.ArrayList<>();
        shapes.add(new Square(3));
        shapes.add(new Rect(2, 5));
        shapes.add(new Shape());
        for (Shape s : shapes) {
            System.out.println(s.describe());
        }
        System.out.println(totalArea(shapes));

        Shape one = new Square(4);
        System.out.println(one.describe());
        System.out.println(one.area());
    }
}
