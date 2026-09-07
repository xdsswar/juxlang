public class Main {
    static String repeatWord(String w, int n) {
        String out = "";
        for (int i = 0; i < n; i++) {
            out = out + w;
            if (i < n - 1) { out = out + "-"; }
        }
        return out;
    }

    public static void main(String[] args) {
        System.out.println(repeatWord("ab", 3));
        System.out.println(repeatWord("x", 1));
        System.out.println(repeatWord("y", 0));

        String csv = "";
        for (int i = 0; i < 5; i++) {
            if (i > 0) { csv = csv + ","; }
            csv = csv + (i * i);
        }
        System.out.println(csv);

        String mixed = "n=" + 42 + " f=" + 1.5 + " b=" + true + " c=" + 'x';
        System.out.println(mixed);

        int n = 7;
        System.out.println("value is " + n + "!");
        System.out.println("" + n + n);
    }
}
