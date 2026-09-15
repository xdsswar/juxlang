import java.util.ArrayList;

public class Main {
    static ArrayList<String> events = new ArrayList<>();

    static String mark(String what) {
        events.add(what);
        return what;
    }

    static class Order {
        static String first = mark("first field");
        static { mark("block"); }
        static String second = mark("second field");
        static int uses = 0;
    }

    static class Settings {
        static String host = mark("host");
        static String port = mark("port");
        static String describe() { return host + ":" + port; }
    }

    static class Counter {
        static int made = 0;
        static { mark("counter block"); }
        Counter() { made = made + 1; }
    }

    public static void main(String[] args) {
        mark("main");
        Order.uses = 1;
        mark("after write " + Order.uses);
        mark(Settings.describe());
        new Counter();
        new Counter();
        mark("made " + Counter.made);
        for (String e : events) {
            System.out.println(e);
        }
    }
}
