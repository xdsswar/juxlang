import java.util.*;
import java.util.concurrent.*;

public class Main {
    record Bag(String name, int[] items) {}

    static int fill(List<Integer> into, int from, int n) {
        for (int i = 0; i < n; i++) { into.add(from + i); }
        return n;
    }

    static <T> T await(Future<T> f) throws Exception { return f.get(); }

    public static void main(String[] args) throws Exception {
        ExecutorService pool = Executors.newFixedThreadPool(4);

        List<Integer> xs = Collections.synchronizedList(new ArrayList<>());
        Future<Integer> a = pool.submit(() -> fill(xs, 0, 100));
        Future<Integer> b = pool.submit(() -> fill(xs, 100, 100));
        Future<Integer> c = pool.submit(() -> { for (int i = 200; i < 300; i++) { xs.add(i); } return 100; });
        Future<Integer> d = pool.submit(() -> { xs.add(-1); return 1; });
        System.out.println(await(a) + await(b) + await(c) + await(d));
        int sum = 0;
        for (int v : xs) { sum += v; }
        System.out.println(xs.size() + " " + sum);

        int[] arr = new int[3];
        Future<Integer> s = pool.submit(() -> { arr[1] = 5; arr[2] = arr[1] * 2; return 0; });
        await(s);
        System.out.println(arr[0] + " " + arr[1] + " " + arr[2]);

        Map<String, Integer> ages = Collections.synchronizedMap(new HashMap<>());
        ages.put("ann", 31);
        Future<Integer> m = pool.submit(() -> { ages.put("bob", 40); return ages.size(); });
        System.out.println(await(m));
        List<String> keys = new ArrayList<>(ages.keySet());
        Collections.sort(keys);
        Integer bob = ages.get("bob");
        System.out.println(keys.get(0) + " " + keys.get(1) + " " + (bob == null ? 0 : bob));

        List<List<Integer>> rows = Collections.synchronizedList(new ArrayList<>());
        rows.add(Collections.synchronizedList(new ArrayList<>()));
        List<Integer> row = rows.get(0);
        Future<Integer> r = pool.submit(() -> { rows.get(0).add(7); rows.add(new ArrayList<>()); return rows.size(); });
        System.out.println(await(r));
        System.out.println(row.size() + " " + row.get(0));

        Bag bag = new Bag("box", new int[2]);
        Future<Integer> g = pool.submit(() -> { bag.items()[0] = 9; return bag.items().length; });
        System.out.println(await(g));
        System.out.println(bag.name() + " " + bag.items()[0]);

        List<Integer> orig = new ArrayList<>();
        orig.add(1);
        Future<Integer> k = pool.submit(() -> { List<Integer> mine = new ArrayList<>(orig); mine.add(2); return mine.size(); });
        System.out.println(await(k) + " " + orig.size());

        pool.shutdown();
    }
}
