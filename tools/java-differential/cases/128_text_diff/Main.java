import java.util.ArrayList;
import java.util.List;

public class Main {
    enum Kind {
        Keep, Add, Remove;

        String mark() {
            return switch (this) {
                case Keep -> " ";
                case Add -> "+";
                case Remove -> "-";
            };
        }
    }

    record Edit(Kind kind, String line) {}

    static int[][] lcsTable(String[] a, String[] b) {
        int[][] t = new int[a.length + 1][b.length + 1];
        for (int i = a.length - 1; i >= 0; i--) {
            for (int j = b.length - 1; j >= 0; j--) {
                if (a[i].equals(b[j])) {
                    t[i][j] = t[i + 1][j + 1] + 1;
                } else {
                    t[i][j] = t[i + 1][j] > t[i][j + 1] ? t[i + 1][j] : t[i][j + 1];
                }
            }
        }
        return t;
    }

    static List<Edit> diff(String[] a, String[] b) {
        int[][] t = lcsTable(a, b);
        List<Edit> edits = new ArrayList<>();
        int i = 0;
        int j = 0;
        while (i < a.length && j < b.length) {
            if (a[i].equals(b[j])) {
                edits.add(new Edit(Kind.Keep, a[i]));
                i++;
                j++;
            } else if (t[i + 1][j] >= t[i][j + 1]) {
                edits.add(new Edit(Kind.Remove, a[i]));
                i++;
            } else {
                edits.add(new Edit(Kind.Add, b[j]));
                j++;
            }
        }
        while (i < a.length) {
            edits.add(new Edit(Kind.Remove, a[i]));
            i++;
        }
        while (j < b.length) {
            edits.add(new Edit(Kind.Add, b[j]));
            j++;
        }
        return edits;
    }

    static String[] lines(String text) {
        List<String> parts = new ArrayList<>();
        String current = "";
        for (int k = 0; k < text.length(); k++) {
            char c = text.charAt(k);
            if (c == '\n') {
                parts.add(current);
                current = "";
            } else {
                current = current + c;
            }
        }
        parts.add(current);
        return parts.toArray(new String[0]);
    }

    static int charLcs(String x, String y) {
        int n = x.length();
        int m = y.length();
        int[][] t = new int[n + 1][m + 1];
        for (int i = 1; i <= n; i++) {
            for (int j = 1; j <= m; j++) {
                if (x.charAt(i - 1) == y.charAt(j - 1)) {
                    t[i][j] = t[i - 1][j - 1] + 1;
                } else {
                    t[i][j] = t[i - 1][j] > t[i][j - 1] ? t[i - 1][j] : t[i][j - 1];
                }
            }
        }
        return t[n][m];
    }

    public static void main(String[] args) {
        String[] before = lines("alpha\nbeta\ngamma\ndelta\nepsilon");
        String[] after = lines("alpha\ngamma\ndelta\nzeta\nepsilon\neta");
        List<Edit> edits = diff(before, after);
        int added = 0;
        int removed = 0;
        for (Edit e : edits) {
            System.out.println(e.kind().mark() + " " + e.line());
            if (e.kind() == Kind.Add) { added++; }
            if (e.kind() == Kind.Remove) { removed++; }
        }
        System.out.println("+" + added + " -" + removed + " of " + edits.size());
        System.out.println(charLcs("ABCBDAB", "BDCABA"));
        System.out.println(charLcs("kitten", "sitting"));
        System.out.println(charLcs("", "abc"));
    }
}
