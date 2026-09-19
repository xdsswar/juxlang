import java.util.ArrayList;
import java.util.List;

public class Main {
    sealed interface Tok permits LBrace, RBrace, LBracket, RBracket, Colon, Comma, Str, Num, Bool, Null, End {}
    record LBrace() implements Tok {}
    record RBrace() implements Tok {}
    record LBracket() implements Tok {}
    record RBracket() implements Tok {}
    record Colon() implements Tok {}
    record Comma() implements Tok {}
    record Str(String s) implements Tok {}
    record Num(long n) implements Tok {}
    record Bool(boolean b) implements Tok {}
    record Null() implements Tok {}
    record End() implements Tok {}

    static class Lexer {
        private String text;
        private int pos = 0;

        Lexer(String text) { this.text = text; }

        private char peek() {
            return pos < text.length() ? text.charAt(pos) : '\0';
        }

        private boolean isDigit(char c) { return c >= '0' && c <= '9'; }
        private boolean isLetter(char c) { return c >= 'a' && c <= 'z'; }

        List<Tok> tokens() {
            List<Tok> out = new ArrayList<>();
            while (pos < text.length()) {
                char c = peek();
                if (c == ' ' || c == '\n') {
                    pos++;
                } else if (c == '{') {
                    out.add(new LBrace()); pos++;
                } else if (c == '}') {
                    out.add(new RBrace()); pos++;
                } else if (c == '[') {
                    out.add(new LBracket()); pos++;
                } else if (c == ']') {
                    out.add(new RBracket()); pos++;
                } else if (c == ':') {
                    out.add(new Colon()); pos++;
                } else if (c == ',') {
                    out.add(new Comma()); pos++;
                } else if (c == '"') {
                    pos++;
                    String s = "";
                    while (peek() != '"') {
                        s += peek();
                        pos++;
                    }
                    pos++;
                    out.add(new Str(s));
                } else if (isDigit(c) || c == '-') {
                    boolean neg = c == '-';
                    if (neg) {
                        pos++;
                    }
                    long n = 0;
                    while (isDigit(peek())) {
                        n = n * 10 + (peek() - '0');
                        pos++;
                    }
                    out.add(new Num(neg ? -n : n));
                } else if (isLetter(c)) {
                    String word = "";
                    while (isLetter(peek())) {
                        word += peek();
                        pos++;
                    }
                    if (word.equals("true")) {
                        out.add(new Bool(true));
                    } else if (word.equals("false")) {
                        out.add(new Bool(false));
                    } else {
                        out.add(new Null());
                    }
                } else {
                    pos++;
                }
            }
            out.add(new End());
            return out;
        }
    }

    sealed interface Value permits JStr, JNum, JBool, JNull, JArr, JObj {}
    record JStr(String s) implements Value {}
    record JNum(long n) implements Value {}
    record JBool(boolean b) implements Value {}
    record JNull() implements Value {}
    record JArr(List<Value> items) implements Value {}
    record JObj(List<String> keys, List<Value> vals) implements Value {}

    static class Parser {
        private List<Tok> toks;
        private int i = 0;

        Parser(List<Tok> toks) { this.toks = toks; }

        private Tok next() {
            Tok t = toks.get(i);
            i++;
            return t;
        }

        private boolean at(Tok t) { return toks.get(i).equals(t); }

        Value value() {
            Tok t = next();
            if (t instanceof Str(String s)) { return new JStr(s); }
            if (t instanceof Num(long n)) { return new JNum(n); }
            if (t instanceof Bool(boolean b)) { return new JBool(b); }
            if (t instanceof LBracket) {
                List<Value> items = new ArrayList<>();
                while (!at(new RBracket())) {
                    items.add(value());
                    if (at(new Comma())) { next(); }
                }
                next();
                return new JArr(items);
            }
            if (t instanceof LBrace) {
                List<String> keys = new ArrayList<>();
                List<Value> vals = new ArrayList<>();
                while (!at(new RBrace())) {
                    Tok key = next();
                    if (key instanceof Str(String k)) {
                        keys.add(k);
                    } else {
                        keys.add("?");
                    }
                    next();
                    vals.add(value());
                    if (at(new Comma())) { next(); }
                }
                next();
                return new JObj(keys, vals);
            }
            return new JNull();
        }
    }

    static String show(Value v) {
        return switch (v) {
            case JStr(String s) -> "\"" + s + "\"";
            case JNum(long n) -> "" + n;
            case JBool(boolean b) -> b ? "true" : "false";
            case JNull() -> "null";
            case JArr(List<Value> items) -> showArr(items);
            case JObj(List<String> keys, List<Value> vals) -> showObj(keys, vals);
        };
    }

    static String showArr(List<Value> items) {
        String out = "[";
        for (int k = 0; k < items.size(); k++) {
            if (k > 0) { out += ","; }
            out += show(items.get(k));
        }
        return out + "]";
    }

    static String showObj(List<String> keys, List<Value> vals) {
        String out = "{";
        for (int k = 0; k < keys.size(); k++) {
            if (k > 0) { out += ","; }
            out += keys.get(k) + ":" + show(vals.get(k));
        }
        return out + "}";
    }

    static long total(Value v) {
        return switch (v) {
            case JNum(long n) -> n;
            case JArr(List<Value> items) -> totalAll(items);
            case JObj(List<String> keys, List<Value> vals) -> totalAll(vals);
            default -> 0;
        };
    }

    static long totalAll(List<Value> items) {
        long sum = 0;
        for (Value item : items) { sum += total(item); }
        return sum;
    }

    public static void main(String[] args) {
        String src = "{\"name\": \"jux\", \"tags\": [\"fast\", \"small\"], \"sizes\": [3, -4, 10],\n"
            + " \"nested\": {\"ok\": true, \"none\": null, \"deep\": [[1, 2], [30]]}}";
        List<Tok> toks = new Lexer(src).tokens();
        System.out.println("tokens: " + toks.size());
        Value tree = new Parser(toks).value();
        System.out.println(show(tree));
        System.out.println("sum of numbers: " + total(tree));
    }
}
