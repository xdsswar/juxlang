public class Main {
    interface Report {
        String title();
        int rows();

        default String header() {
            return "== " + this.title() + " ==";
        }

        default String render() {
            return this.header() + " (" + this.rows() + " rows)";
        }
    }

    static class Sales implements Report {
        public String title() { return "sales"; }
        public int rows() { return 12; }
    }

    static class Empty implements Report {
        public String title() { return "empty"; }
        public int rows() { return 0; }
        public String render() { return "no data for " + this.title(); }
    }

    static class Base {
        String step() { return "base-step"; }
        String run() { return "run:" + this.step(); }
    }

    static class Mid extends Base {
        String step() { return "mid-step"; }
    }

    static class Leaf extends Mid {
        String step() { return "leaf-step(" + super.step() + ")"; }
    }

    static void printAll(java.util.List<Report> rs) {
        for (Report r : rs) {
            System.out.println(r.render());
        }
    }

    public static void main(String[] args) {
        java.util.List<Report> rs = new java.util.ArrayList<>();
        rs.add(new Sales());
        rs.add(new Empty());
        printAll(rs);

        System.out.println(new Sales().header());
        System.out.println(new Leaf().run());
        System.out.println(new Mid().run());
        System.out.println(new Base().run());
    }
}
