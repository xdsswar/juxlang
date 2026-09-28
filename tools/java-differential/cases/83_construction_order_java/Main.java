import java.util.ArrayList;

public class Main {
    static ArrayList<String> events = new ArrayList<>();

    static int mark(String what) {
        events.add(what);
        return events.size();
    }

    static class Tag {
        String text;
        Tag(String text) { this.text = text; }
    }

    static class Root {
        int r1 = mark("root.r1");
        { mark("root.init"); }
        int r2 = mark("root.r2");

        Root() {
            mark("root.body");
            mark("root sees: " + describe());
        }

        String describe() { return "root"; }
    }

    static class Middle extends Root {
        int m1 = mark("middle.m1");
        int seen = 42;
        Tag tag = new Tag("middle-tag");

        Middle() {
            super();
            mark("middle.body seen=" + seen);
        }

        Middle(int extra) {
            this();
            seen += extra;
            mark("middle.extra seen=" + seen);
        }

        @Override
        String describe() {
            String t;
            try {
                t = tag.text;
            } catch (NullPointerException e) {
                t = "unset";
            }
            return "middle seen=" + seen + " tag=" + t;
        }
    }

    static class Leaf extends Middle {
        { mark("leaf.init"); }
        int l1 = mark("leaf.l1");

        Leaf(int extra) {
            super(extra);
            mark("leaf.body");
        }
    }

    public static void main(String[] args) {
        Leaf leaf = new Leaf(5);
        for (String e : events) {
            System.out.println(e);
        }
        System.out.println(leaf.describe());
        System.out.println(leaf.r1 + " " + leaf.r2 + " " + leaf.m1 + " " + leaf.l1);
    }
}
