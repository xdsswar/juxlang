import java.util.*;

public class Main {
    static class Board {
        private final List<Integer> nums = Collections.synchronizedList(new ArrayList<>());
        private final Map<String, Integer> tally = Collections.synchronizedMap(new HashMap<>());
        private final List<String> inbox;

        Board(List<String> inbox) { this.inbox = inbox; }

        void record(String who, int v) {
            this.nums.add(v);
            Integer now = this.tally.get(who);
            this.tally.put(who, (now == null ? 0 : now) + 1);
            this.inbox.add(who);
        }

        void stamp(Note n) { n.lines.add("stamped " + this.nums.size()); }

        List<Integer> getNums() { return this.nums; }
        Map<String, Integer> getTally() { return this.tally; }
        List<String> getInbox() { return this.inbox; }
    }

    static class Note {
        List<String> lines = new ArrayList<>();
    }

    static List<Integer> evens(Board b) {
        List<Integer> out = new ArrayList<>();
        for (int v : new ArrayList<>(b.getNums())) {
            if (v % 2 == 0) { out.add(v); }
        }
        return out;
    }

    static int fill(Board b, String who, int from, int n) {
        for (int i = 0; i < n; i++) {
            b.record(who, from + i);
        }
        b.getNums().add(1000);
        return n;
    }

    static int[] results = new int[4];

    public static void main(String[] args) throws Exception {
        List<String> inbox = Collections.synchronizedList(new ArrayList<>());
        inbox.add("start");
        Board b = new Board(inbox);

        Thread w1 = new Thread(() -> results[0] = fill(b, "ann", 0, 250));
        Thread w2 = new Thread(() -> results[1] = fill(b, "bob", 1000, 250));
        Thread w3 = new Thread(() -> results[2] = fill(b, "cy", 2000, 250));
        Thread w4 = new Thread(() -> results[3] = fill(b, "dee", 3000, 250));
        w1.start(); w2.start(); w3.start(); w4.start();
        w1.join(); w2.join(); w3.join(); w4.join();
        System.out.println(results[0] + results[1] + results[2] + results[3]);

        List<Integer> nums = b.getNums();
        System.out.println(nums.size());
        int sum = 0;
        for (int v : nums) { sum += v; }
        System.out.println(sum);

        List<String> keys = new ArrayList<>(b.getTally().keySet());
        Collections.sort(keys);
        for (String k : keys) {
            Integer c = b.getTally().get(k);
            System.out.println(k + " " + (c == null ? 0 : c));
        }

        System.out.println(inbox.size());
        inbox.add("end");
        System.out.println(b.getInbox().size());
        System.out.println(b.getInbox().get(0) + " " + b.getInbox().get(inbox.size() - 1));

        nums.add(7);
        System.out.println(b.getNums().size());

        Note note = new Note();
        b.stamp(note);
        System.out.println(note.lines.get(0));

        List<Integer> ev = java.util.concurrent.CompletableFuture.supplyAsync(() -> evens(b)).get();
        System.out.println(ev.size());
        System.out.println((b.getNums() == nums) + " " + (b.getNums() == (Object) ev));
    }
}
