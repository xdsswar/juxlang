import java.util.ArrayList;
import java.util.function.Consumer;
import java.util.function.IntUnaryOperator;

public class Main {
    static class Counter {
        int hits = 0;
        void hit() { hits = hits + 1; }
    }

    static class Bus {
        private ArrayList<Consumer<String>> handlers = new ArrayList<>();
        private ArrayList<String> log = new ArrayList<>();

        void on(Consumer<String> h) { handlers.add(h); }

        void emit(String event) {
            log.add(event);
            for (Consumer<String> h : handlers) {
                h.accept(event);
            }
        }

        void clear() { handlers.clear(); }
        int handlerCount() { return handlers.size(); }
        int logged() { return log.size(); }
    }

    public static void main(String[] args) {
        Bus bus = new Bus();
        Counter counter = new Counter();
        ArrayList<String> seen = new ArrayList<>();

        bus.on(e -> counter.hit());
        bus.on(e -> seen.add(e.toUpperCase()));
        bus.on(e -> {
            if (e.startsWith("err")) {
                System.out.println("alarm: " + e);
            }
        });

        bus.emit("start");
        bus.emit("error-disk");
        bus.emit("tick");
        System.out.println(counter.hits);
        System.out.println(seen.size());
        for (String s : seen) {
            System.out.println(s);
        }

        System.out.println(bus.handlerCount());
        bus.clear();
        bus.emit("after-clear");
        System.out.println(counter.hits);
        System.out.println(bus.logged());

        ArrayList<IntUnaryOperator> steps = new ArrayList<>();
        steps.add(x -> x + 3);
        steps.add(x -> x * 2);
        steps.add(x -> x - 1);
        int v = 5;
        for (IntUnaryOperator f : steps) {
            v = f.applyAsInt(v);
        }
        System.out.println(v);
    }
}
