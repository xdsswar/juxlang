import java.util.*;

public class Main {
    static <T, U> int index(TreeMap<T, Integer> into, TreeSet<T> seen, T a, T b, T c, U tag, int depth) {
        if (depth > 0) {
            List<U> wrapped = new ArrayList<>();
            wrapped.add(tag);
            return index(into, seen, a, b, c, wrapped, depth - 1);
        }
        into.put(c, 3);
        into.put(a, 1);
        into.put(b, 2);
        seen.add(b);
        seen.add(c);
        seen.add(a);
        seen.add(b);
        int order = 0;
        for (Map.Entry<T, Integer> e : into.entrySet()) {
            order = order * 10 + e.getValue();
        }
        return order * 10 + seen.size();
    }

    public static void main(String[] args) {
        TreeMap<String, Integer> byName = new TreeMap<>();
        TreeSet<String> names = new TreeSet<>();
        System.out.println(index(byName, names, "cid", "ann", "bob", 0, 2));
        for (Map.Entry<String, Integer> e : byName.entrySet()) {
            System.out.println(e.getKey() + "=" + e.getValue());
        }
        System.out.println(names.size());
        TreeMap<Integer, Integer> byNum = new TreeMap<>();
        System.out.println(index(byNum, new TreeSet<Integer>(), 30, 10, 20, 0, 3));
        System.out.println(byNum.size());
    }
}
