import java.util.ArrayList;

public class Main {
    interface Listener<E> {
        void on(E event);
    }

    record Order(String id, long cents) {}
    record Refund(String id, long cents) {}

    static class Bus<E> {
        private ArrayList<Listener<E>> listeners = new ArrayList<>();
        int delivered = 0;

        void subscribe(Listener<E> l) {
            listeners.add(l);
        }

        void publish(E event) {
            for (Listener<E> l : listeners) {
                l.on(event);
                delivered++;
            }
        }
    }

    static class Ledger implements Listener<Order> {
        long total = 0;
        ArrayList<String> ids = new ArrayList<>();

        public void on(Order o) {
            total += o.cents();
            ids.add(o.id());
        }
    }

    static class Stats {
        static int created = 0;
        static String first = "unset";
        static final int LIMIT = 3;

        static class Counter {
            int n = 0;
            void hit() { n++; }
        }

        static Counter counter = new Counter();

        static void note(String what) {
            created++;
            if (first.equals("unset")) {
                first = what;
            }
            counter.hit();
        }
    }

    static class Alarm implements Listener<Refund> {
        int raised = 0;

        public void on(Refund r) {
            if (r.cents() > 5000) {
                raised++;
                System.out.println("alarm: large refund " + r.id());
            }
            Stats.note("refund " + r.id());
        }
    }

    public static void main(String[] args) {
        Bus<Order> orders = new Bus<>();
        Ledger ledger = new Ledger();
        orders.subscribe(ledger);
        ArrayList<String> big = new ArrayList<>();
        orders.subscribe((o) -> {
            if (o.cents() >= 10000) {
                big.add(o.id());
            }
            Stats.note("order " + o.id());
        });

        Bus<Refund> refunds = new Bus<>();
        Alarm alarm = new Alarm();
        refunds.subscribe(alarm);

        orders.publish(new Order("A1", 2500));
        orders.publish(new Order("A2", 12000));
        refunds.publish(new Refund("R1", 800));
        orders.publish(new Order("A3", 15000));
        refunds.publish(new Refund("R2", 9000));

        System.out.println("ledger total " + ledger.total + " from " + ledger.ids.size() + " orders");
        String bigLine = "";
        for (String id : big) {
            bigLine += id + " ";
        }
        System.out.println("big orders: " + bigLine.trim());
        System.out.println("alarms " + alarm.raised);
        System.out.println("delivered " + orders.delivered + " + " + refunds.delivered);
        System.out.println("stats created=" + Stats.created + " first=" + Stats.first + " counter=" + Stats.counter.n);
        System.out.println("over limit: " + (Stats.created > Stats.LIMIT));
    }
}
