// Twin of 121_interp_inferred_double.jux. Java has no string interpolation,
// so `"" + x` stands in for `$"$x"`; the question the harness asks is what
// each value RENDERS as, and Java is the oracle for that.
public class Main {
    public static void main(String[] args) {
        double d = 1.0;
        var v = 2.0;
        System.out.println("" + d);
        System.out.println("" + v);
        System.out.println("" + v);
        System.out.println(v);

        var sum = 1.0 + 2;
        System.out.println("" + sum);
        var half = 1 / 2.0;
        System.out.println("" + half);
        var whole = 6.0 / 2.0;
        System.out.println("" + whole);

        var n = 5;
        System.out.println("" + n);
        var text = "hi";
        System.out.println(text);
        System.out.println(v + " " + n + " " + text);
    }
}
