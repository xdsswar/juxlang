import java.util.ArrayList;

public class Main {
    record Point(int x, int y) {
        int sum() { return this.x + this.y; }
    }
    record Line(Point from, Point to) {
        int span() { return this.to.x() - this.from.x(); }
    }

    public static void main(String[] args) {
        Point p = new Point(1, 2);
        System.out.println(p.x());
        System.out.println(p.y());
        System.out.println(p.sum());

        Point q = new Point(1, 2);
        Point r = new Point(9, 9);
        System.out.println(p.equals(q));
        System.out.println(p.equals(r));

        Line line = new Line(new Point(0, 0), new Point(10, 4));
        System.out.println(line.span());
        System.out.println(line.from().x());
        System.out.println(line.to().y());

        ArrayList<Point> pts = new ArrayList<>();
        pts.add(new Point(3, 4));
        pts.add(new Point(5, 6));
        System.out.println(pts.size());
        System.out.println(pts.get(1).sum());
    }
}
