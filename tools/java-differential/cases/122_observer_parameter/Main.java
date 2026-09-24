// Twin of 122_observer_parameter.jux. Java has no observable properties, so
// `Meter` carries the listener list by hand and fires it from its setter.
// The question the harness asks is the same: does an observer handed to a
// function through a parameter actually get registered and fire?
import java.util.ArrayList;
import java.util.List;
import java.util.function.BiConsumer;

public class Main {
    static class Meter {
        private int value = 0;
        private final List<BiConsumer<Integer, Integer>> observers = new ArrayList<>();

        void setV(int now) {
            int old = value;
            if (old != now) {
                value = now;
                for (BiConsumer<Integer, Integer> o : observers) {
                    o.accept(old, now);
                }
            }
        }

        void attach(BiConsumer<Integer, Integer> o) {
            observers.add(o);
        }

        int size() {
            return observers.size();
        }
    }

    static void wire(Meter m, BiConsumer<Integer, Integer> o) {
        m.attach(o);
    }

    public static void main(String[] args) {
        Meter named = new Meter();
        BiConsumer<Integer, Integer> watch =
                (old, now) -> System.out.println("named " + old + " -> " + now);
        wire(named, watch);
        named.setV(3);
        System.out.println("named size " + named.size());

        Meter inline = new Meter();
        wire(inline, (old, now) -> System.out.println("inline " + old + " -> " + now));
        inline.setV(7);
        System.out.println("inline size " + inline.size());

        Meter a = new Meter();
        Meter b = new Meter();
        wire(a, watch);
        wire(b, watch);
        a.setV(1);
        b.setV(2);
        System.out.println("shared sizes " + a.size() + " " + b.size());
    }
}
