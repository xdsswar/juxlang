import java.util.*;

public class Main {
    static class Registry {
        private final List<String> log = Collections.synchronizedList(new ArrayList<>());
        List<String> items = Collections.synchronizedList(new ArrayList<>());
        void handle(String s) { this.log.add(s); }
        void addAll(List<String> more) { for (String m : more) { this.log.add(m); } }
        List<String> getLog() { return this.log; }
        List<String> fresh() { List<String> v = new ArrayList<>(); v.add("f"); return v; }
        int count() { return this.log.size(); }
    }

    static int work(Registry r, String tag, int n) {
        for (int i = 0; i < n; i++) { r.handle(tag); }
        r.items.add(tag);
        return n;
    }

    public static void main(String[] args) throws Exception {
        Registry r = new Registry();
        Thread a = new Thread(() -> work(r, "a", 3));
        Thread b = new Thread(() -> work(r, "b", 2));
        a.start();
        b.start();
        a.join();
        b.join();
        System.out.println(r.count() + " " + r.items.size());

        List<String> more = new ArrayList<>();
        more.add("c");
        more.add("d");
        r.addAll(more);
        List<String> log = r.getLog();
        System.out.println(log.size());
        int total = 0;
        for (String s : log) { total += s.length(); }
        System.out.println(total);
        log.add("z");
        System.out.println(r.count());

        r.items.add("main");
        List<String> items = r.items;
        System.out.println(items.size() + " " + r.items.size());
        items.add("alias");
        System.out.println(r.items.size());

        r.items = more;
        System.out.println(r.items.size() + " " + more.size());
        more.add("e");
        r.items.add("f");
        System.out.println(r.items.size() + " " + more.size());

        List<String> f = r.fresh();
        f.add("mine");
        List<String> c = new ArrayList<>(r.getLog());
        c.add("extra");
        System.out.println(f.size() + " " + c.size() + " " + r.count());
    }
}
