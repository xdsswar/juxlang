import java.util.HashMap;

public class Main {
    static class ParseError extends Exception {
        ParseError(String message) { super(message); }
    }

    static class Calc {
        private String src = "";
        private int pos = 0;
        private HashMap<String, Integer> vars = new HashMap<>();

        private char peek() {
            while (pos < src.length() && src.charAt(pos) == ' ') { pos = pos + 1; }
            return pos < src.length() ? src.charAt(pos) : '\0';
        }

        private boolean isDigit(char c) { return c >= '0' && c <= '9'; }
        private boolean isLetter(char c) { return c >= 'a' && c <= 'z'; }

        private int number() {
            int v = 0;
            while (pos < src.length() && isDigit(src.charAt(pos))) {
                v = v * 10 + ((int) src.charAt(pos) - (int) '0');
                pos = pos + 1;
            }
            return v;
        }

        private String name() {
            int start = pos;
            while (pos < src.length() && isLetter(src.charAt(pos))) { pos = pos + 1; }
            return src.substring(start, pos);
        }

        private int primary() throws ParseError {
            char c = peek();
            if (c == '(') {
                pos = pos + 1;
                int v = expr();
                if (peek() != ')') { throw new ParseError("expected ) at " + pos); }
                pos = pos + 1;
                return v;
            }
            if (c == '-') {
                pos = pos + 1;
                return -primary();
            }
            if (isDigit(c)) { return number(); }
            if (isLetter(c)) {
                String n = name();
                if (!vars.containsKey(n)) { throw new ParseError("unknown variable " + n); }
                return vars.get(n);
            }
            throw new ParseError("unexpected '" + c + "' at " + pos);
        }

        private int term() throws ParseError {
            int v = primary();
            while (true) {
                char c = peek();
                if (c == '*') { pos = pos + 1; v = v * primary(); }
                else if (c == '/') {
                    pos = pos + 1;
                    int d = primary();
                    if (d == 0) { throw new ParseError("division by zero"); }
                    v = v / d;
                }
                else if (c == '%') { pos = pos + 1; v = v % primary(); }
                else { return v; }
            }
        }

        private int expr() throws ParseError {
            int v = term();
            while (true) {
                char c = peek();
                if (c == '+') { pos = pos + 1; v = v + term(); }
                else if (c == '-') { pos = pos + 1; v = v - term(); }
                else { return v; }
            }
        }

        String run(String line) {
            src = line;
            pos = 0;
            try {
                int eq = line.indexOf("=");
                if (eq > 0) {
                    String target = line.substring(0, eq).trim();
                    src = line.substring(eq + 1);
                    int v = expr();
                    vars.put(target, v);
                    return target + " = " + v;
                }
                int v = expr();
                if (peek() != '\0') { throw new ParseError("trailing input at " + pos); }
                return "" + v;
            } catch (ParseError e) {
                return "error: " + e.getMessage();
            }
        }
    }

    public static void main(String[] args) {
        Calc c = new Calc();
        String[] lines = {
            "1 + 2 * 3",
            "(1 + 2) * 3",
            "x = 7 * 6",
            "x / 5",
            "x % 5 - -3",
            "y + 1",
            "10 / (5 - 5)",
            "2 * (3 + 4",
            "8 - 3 - 2",
            "100 / 7 / 2",
            "4 $ 4",
        };
        for (String l : lines) {
            System.out.println(l + "  =>  " + c.run(l));
        }
    }
}
