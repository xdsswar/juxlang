public class Main {
    static String show(int v) { return "int:" + v; }
    static String show(double v) { return "double:" + v; }
    static String show(String v) { return "string:" + v; }
    static String show(int a, int b) { return "two:" + (a + b); }

    static class Fmt {
        String of(int v) { return "m-int:" + v; }
        String of(String v) { return "m-string:" + v; }
        String of(int a, String b) { return "m-both:" + a + b; }
    }

    public static void main(String[] args) {
        System.out.println(show(1));
        System.out.println(show(2.5));
        System.out.println(show("x"));
        System.out.println(show(3, 4));

        Fmt f = new Fmt();
        System.out.println(f.of(7));
        System.out.println(f.of("y"));
        System.out.println(f.of(1, "z"));

        int n = 9;
        String s = "s";
        System.out.println(show(n));
        System.out.println(show(s));
        System.out.println(f.of(n, s));
    }
}
