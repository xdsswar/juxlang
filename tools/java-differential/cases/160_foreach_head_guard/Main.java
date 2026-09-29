import java.util.*;

public class Main {
    static class Scanner {
        private String text;
        private int seen = 0;
        private List<String> words = new ArrayList<>();

        Scanner(String text) { this.text = text; }

        private void count(String w) {
            this.seen = this.seen + 1;
            this.words.add(w);
        }

        int scan() {
            for (String line : this.text.lines().toList()) {
                final String w = line.trim();
                if (w.length() > 0) {
                    this.count(w);
                }
                this.text = this.text + "";
            }
            return this.seen;
        }

        int firstLong() {
            int found = 0;
            outer:
            for (String line : text.lines().toList()) {
                for (String w : this.words) {
                    if (w.length() > 4) {
                        found = found + 1;
                        continue outer;
                    }
                }
            }
            return found;
        }

        String joined() {
            String out = "";
            for (String w : this.words) { out = out + w + "."; }
            return out;
        }
    }

    public static void main(String[] args) {
        Scanner s = new Scanner("alpha beta\n  gamma delta epsilon \n\nzeta");
        System.out.println(s.scan());
        System.out.println(s.joined());
        System.out.println(s.firstLong());
    }
}
