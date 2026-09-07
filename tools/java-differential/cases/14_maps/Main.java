import java.util.ArrayList;
import java.util.Collections;
import java.util.HashMap;

public class Main {
    public static void main(String[] args) {
        HashMap<String, Integer> m = new HashMap<>();
        m.put("one", 1);
        m.put("two", 2);
        m.put("three", 3);
        System.out.println(m.size());
        System.out.println(m.get("two"));
        System.out.println(m.get("nope"));
        System.out.println(m.containsKey("one"));
        System.out.println(m.containsKey("zzz"));

        m.put("one", 11);
        System.out.println(m.get("one"));
        System.out.println(m.size());

        ArrayList<String> keys = new ArrayList<>(m.keySet());
        Collections.sort(keys);
        for (String k : keys) {
            System.out.println(k);
        }
    }
}
