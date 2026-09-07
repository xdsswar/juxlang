import java.util.ArrayList;
import java.util.HashMap;

public class Main {
    static class Wrap<T> {
        private T v;
        Wrap(T v) { this.v = v; }
        T get() { return this.v; }
    }

    public static void main(String[] args) {
        ArrayList<ArrayList<Integer>> rows = new ArrayList<>();
        for (int r = 0; r < 3; r++) {
            ArrayList<Integer> row = new ArrayList<>();
            for (int c = 0; c < 3; c++) {
                row.add(r * 3 + c);
            }
            rows.add(row);
        }
        System.out.println(rows.size());
        System.out.println(rows.get(0).get(0));
        System.out.println(rows.get(2).get(2));
        System.out.println(rows.get(1).get(2));

        int total = 0;
        for (ArrayList<Integer> row : rows) {
            for (int v : row) {
                total = total + v;
            }
        }
        System.out.println(total);

        ArrayList<Wrap<String>> wraps = new ArrayList<>();
        wraps.add(new Wrap<>("a"));
        wraps.add(new Wrap<>("b"));
        System.out.println(wraps.size());
        System.out.println(wraps.get(0).get());
        System.out.println(wraps.get(1).get());

        Wrap<ArrayList<Integer>> wrapped = new Wrap<>(new ArrayList<>());
        wrapped.get().add(7);
        wrapped.get().add(8);
        System.out.println(wrapped.get().size());
        System.out.println(wrapped.get().get(1));

        HashMap<String, ArrayList<Integer>> m = new HashMap<>();
        ArrayList<Integer> xs = new ArrayList<>();
        xs.add(1);
        xs.add(2);
        m.put("k", xs);
        System.out.println(m.size());
        System.out.println(m.get("k").size());
    }
}
