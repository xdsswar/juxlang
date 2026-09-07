import java.util.ArrayList;

public class Main {
    interface Shape {
        double area();
        String kind();
    }
    static class Circle implements Shape {
        private double r;
        Circle(double r) { this.r = r; }
        public double area() { return 3.14159 * this.r * this.r; }
        public String kind() { return "circle"; }
    }
    static class Rect implements Shape {
        private double w;
        private double h;
        Rect(double w, double h) { this.w = w; this.h = h; }
        public double area() { return this.w * this.h; }
        public String kind() { return "rect"; }
    }

    public static void main(String[] args) {
        ArrayList<Shape> shapes = new ArrayList<>();
        shapes.add(new Circle(1.0));
        shapes.add(new Rect(2.0, 3.0));
        shapes.add(new Circle(2.0));

        double total = 0.0;
        for (Shape s : shapes) {
            System.out.println(s.kind() + " " + s.area());
            total = total + s.area();
        }
        System.out.println(total);
        System.out.println(shapes.size());
    }
}
