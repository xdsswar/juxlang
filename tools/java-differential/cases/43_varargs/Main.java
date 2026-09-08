public class Main {
    static int total(int... xs) {
        int t = 0;
        for (int x : xs) {
            t = t + x;
        }
        return t;
    }

    static String join(String sep, String... parts) {
        String out = "";
        for (int i = 0; i < parts.length; i++) {
            if (i > 0) { out = out + sep; }
            out = out + parts[i];
        }
        return out;
    }

    public static void main(String[] args) {
        System.out.println(total());
        System.out.println(total(1));
        System.out.println(total(1, 2, 3));
        System.out.println(total(1, 2, 3, 4, 5));
        System.out.println(join("-"));
        System.out.println(join("-", "a"));
        System.out.println(join("-", "a", "b", "c"));
        System.out.println(join(", ", "x", "y"));
    }
}
