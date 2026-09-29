import java.util.*;
import java.util.function.Supplier;

public class Main {
    enum Kind { Mine, Fresh }

    static class Holder {
        List<Integer> maybe = null;
        List<Integer> items = new ArrayList<>();
        void keep(List<Integer> xs) { this.maybe = xs; }
        List<Integer> kept() { return this.maybe; }
    }

    static void addOne(List<Integer> xs, int x) { xs.add(x); }
    static List<Integer> handBack(List<Integer> xs) { return xs; }

    static int sumOf(List<Integer> xs) {
        int total = 0;
        for (int x : xs) { total += x; }
        return total;
    }

    public static void main(String[] args) {
        List<Integer> v = new ArrayList<>();

        List<Integer> local = v;
        local.add(1);
        System.out.println("nullable local: " + v.size());

        addOne(v, 2);
        System.out.println("nullable parameter: " + v.size());

        handBack(v).add(3);
        System.out.println("nullable return: " + v.size());

        Holder h = new Holder();
        h.keep(v);
        h.kept().add(4);
        System.out.println("nullable field via method: " + v.size());

        h.maybe = v;
        h.maybe.add(5);
        System.out.println("nullable field: " + v.size());

        h.items = v;
        h.items.add(6);
        System.out.println("field: " + v.size());

        List<Integer> w = new ArrayList<>();
        w = v;
        w.add(7);
        System.out.println("reassignment: " + v.size());

        List<Integer> nw = null;
        nw = v;
        nw.add(8);
        System.out.println("nullable reassignment: " + v.size());

        Kind k = Kind.Mine;
        List<Integer> picked = switch (k) {
            case Mine -> v;
            case Fresh -> new ArrayList<>();
        };
        picked.add(9);
        System.out.println("switch arm: " + v.size());

        Supplier<List<Integer>> supplier = () -> v;
        supplier.get().add(10);
        supplier.get().add(11);
        System.out.println("lambda result: " + v.size());

        Map<String, List<Integer>> byKey = new HashMap<>();
        byKey.put("v", v);
        byKey.get("v").add(12);
        System.out.println("map value: " + v.size());

        List<List<Integer>> outer = new ArrayList<>();
        outer.add(v);
        outer.get(0).add(13);
        System.out.println("list element: " + v.size());

        System.out.println("sum: " + sumOf(local));
    }
}
