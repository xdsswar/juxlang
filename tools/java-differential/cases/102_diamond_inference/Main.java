import java.util.ArrayList;
import java.util.HashMap;

// Twin of 90_diamond_inference.jux: the diamond takes its type arguments
// from the target slot (local, field, assignment, return, argument).
public class Main {
    static class Box<T> {
        private T item;
        Box() { item = null; }
        void put(T t) { item = t; }
        T get() { return item; }
    }

    static class Registry {
        private HashMap<String, ArrayList<Integer>> groups = new HashMap<>();

        void add(String key, int value) {
            if (!groups.containsKey(key)) {
                groups.put(key, new ArrayList<>());
            }
        }

        int size() { return groups.size(); }
    }

    static ArrayList<String> names() {
        ArrayList<String> out = new ArrayList<>();
        out.add("ann");
        out.add("bob");
        return out;
    }

    static ArrayList<Integer> fresh() { return new ArrayList<>(); }

    static int count(ArrayList<Integer> xs) { return xs.size(); }

    public static void main(String[] args) {
        Box<String> box = new Box<>();
        box.put("hi");
        System.out.println(box.get());

        ArrayList<Integer> v;
        v = new ArrayList<>();
        v.add(3);
        v.add(4);
        System.out.println(v.size());

        System.out.println(names().size());
        System.out.println(fresh().size());
        System.out.println(count(new ArrayList<>()));

        Registry registry = new Registry();
        registry.add("x", 1);
        registry.add("y", 2);
        System.out.println(registry.size());
    }
}
