import java.util.ArrayList;

public class Main {
    static ArrayList<String> events = new ArrayList<>();

    static String mark(String what) {
        events.add(what);
        return what;
    }

    static class Base {
        protected String note = mark("base-field");
        Base() {
            mark("base-ctor");
        }
    }
    static class Derived extends Base {
        private String own = mark("derived-field");
        Derived() {
            super();
            mark("derived-ctor");
        }
        String describe() { return this.note + "/" + this.own; }
    }

    public static void main(String[] args) {
        Derived d = new Derived();
        for (String e : events) {
            System.out.println(e);
        }
        System.out.println(d.describe());
        System.out.println(events.size());
    }
}
