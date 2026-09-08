import java.util.HashMap;
import java.util.Map;

public class Main {
    static class Tally {
        private Map<String, Integer> counts = new HashMap<>();

        void add(String key) {
            Integer seen = counts.get(key);
            if (seen == null) {
                counts.put(key, 1);
            } else {
                counts.put(key, seen + 1);
            }
        }

        int countOf(String key) {
            Integer v = counts.get(key);
            if (v == null) {
                return 0;
            }
            return v;
        }

        int size() { return counts.size(); }
        boolean has(String key) { return counts.containsKey(key); }
    }

    public static void main(String[] args) {
        Tally t = new Tally();
        t.add("pear");
        t.add("apple");
        t.add("pear");
        t.add("fig");
        t.add("pear");
        t.add("apple");

        System.out.println(t.countOf("apple"));
        System.out.println(t.countOf("pear"));
        System.out.println(t.countOf("fig"));
        System.out.println(t.countOf("absent"));
        System.out.println(t.size());
        System.out.println(t.has("fig"));
        System.out.println(t.has("plum"));

        Map<String, String> m = new HashMap<>();
        m.put("a", "alpha");
        m.put("b", "beta");
        m.put("a", "again");
        System.out.println(m.size());
        System.out.println(m.get("a"));
        System.out.println(m.get("b"));

        Map<String, java.util.List<Integer>> nested = new HashMap<>();
        java.util.List<Integer> xs = new java.util.ArrayList<>();
        xs.add(1);
        xs.add(2);
        nested.put("nums", xs);
        System.out.println(nested.get("nums").size());
        System.out.println(nested.get("nums").get(1));
    }
}
