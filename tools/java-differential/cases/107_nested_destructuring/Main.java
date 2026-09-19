public class Main {
    record Pt(int x, int y) {}
    record Line(Pt start, Pt end) {}
    record Named(String label, Line line) {}

    public static void main(String[] args) {
        var l = new Line(new Pt(1, 2), new Pt(3, 4));

        if (l instanceof Line(Pt(var x1, var y1), var end)) {
            System.out.println(x1 + y1 + end.x());
        }

        var n = new Named("diag", new Line(new Pt(0, 0), new Pt(5, 5)));
        if (n instanceof Named(var label, Line(Pt(var ignoredX, var sy), Pt(var ex, var ignoredY)))) {
            System.out.println(label + " " + sy + " " + ex);
        }

        if (l instanceof Line(Pt(var ax, var ay), Pt(var bx, var by))) {
            System.out.println((bx - ax) * (by - ay));
        }
    }
}
