public class Main {
    static class Box {
        String label;
    }

    static String describe(String s) {
        if (s == null) { return "none"; }
        return "some:" + s;
    }

    public static void main(String[] args) {
        System.out.println(describe(null));
        System.out.println(describe("hi"));

        Box b = new Box();
        System.out.println(b.label == null);
        System.out.println(b.label == null ? "default" : b.label);
        b.label = "set";
        System.out.println(b.label == null);
        System.out.println(b.label == null ? "default" : b.label);
        System.out.println(b.label.length());

        String maybe = null;
        System.out.println(maybe == null);
        maybe = "abcd";
        System.out.println(maybe.length());
    }
}
