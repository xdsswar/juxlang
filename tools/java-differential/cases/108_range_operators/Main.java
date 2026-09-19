import java.util.ArrayList;

public class Main {
    record Date(int day) {
        ArrayList<Date> until(Date end) {
            ArrayList<Date> out = new ArrayList<>();
            for (int d = day; d < end.day; d++) {
                out.add(new Date(d));
            }
            return out;
        }

        ArrayList<Date> through(Date end) {
            ArrayList<Date> out = new ArrayList<>();
            for (int d = day; d <= end.day; d++) {
                out.add(new Date(d));
            }
            return out;
        }
    }

    public static void main(String[] args) {
        ArrayList<Date> span = new Date(3).until(new Date(7));
        System.out.println(span.size());
        for (Date d : span) {
            System.out.println(d.day);
        }
        ArrayList<Date> closed = new Date(5).through(new Date(6));
        System.out.println(closed.size());
        System.out.println(closed.get(1).day);
        ArrayList<Date> empty = new Date(9).until(new Date(9));
        System.out.println(empty.size());
    }
}
