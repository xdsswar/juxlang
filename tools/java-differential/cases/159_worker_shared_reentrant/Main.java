import java.util.*;

public class Main {
    static class Pair {
        List<Integer> xs = Collections.synchronizedList(new ArrayList<>());
        List<Integer> log = Collections.synchronizedList(new ArrayList<>());
        int[] cells = new int[4];

        Pair() {
            this.xs.add(3);
            this.xs.add(4);
        }

        synchronized int both() { return this.xs.get(0) + this.xs.get(1); }
        synchronized int corners() { return this.cells[0] + this.cells[3]; }

        synchronized int weight(int v) { return v * this.xs.size(); }
        synchronized int weighed() {
            int total = 0;
            for (int v : new ArrayList<>(this.xs)) { total += this.weight(v); }
            return total;
        }

        synchronized void grow() { this.xs.add(this.xs.get(0) + this.xs.get(1)); }
        synchronized void bump() {
            this.cells[0] = this.cells[0] + 1;
            this.cells[3] = this.cells[0] + this.cells[3];
        }
    }

    static int run(Pair p, int n) {
        int seen = 0;
        for (int i = 0; i < n; i++) {
            seen += p.both() + p.xs.get(0) + p.xs.get(1);
            p.log.add(p.xs.get(0) * p.xs.get(1));
        }
        return seen;
    }

    static int[] results = new int[3];

    public static void main(String[] args) throws Exception {
        Pair p = new Pair();
        System.out.println(p.both());
        System.out.println(p.xs.get(0) + p.xs.get(1) + p.xs.size());
        System.out.println(p.weighed());

        Thread a = new Thread(() -> results[0] = run(p, 100));
        Thread b = new Thread(() -> results[1] = run(p, 100));
        Thread c = new Thread(() -> results[2] = run(p, 100));
        a.start(); b.start(); c.start();
        a.join(); b.join(); c.join();
        System.out.println(results[0] + results[1] + results[2]);
        int sum = 0;
        for (int v : new ArrayList<>(p.log)) { sum += v; }
        System.out.println(p.log.size() + " " + sum);

        p.bump();
        p.bump();
        System.out.println(p.corners() + " " + p.cells[0] * p.cells[3]);

        p.grow();
        p.grow();
        System.out.println(p.xs.size() + " " + p.xs.get(2) + " " + p.xs.get(3));
        System.out.println(p.both() + p.xs.get(p.xs.size() - 1));
    }
}
