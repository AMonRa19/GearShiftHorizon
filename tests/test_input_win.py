import ctypes

from gearshift.input_win import INPUT


def test_windows_input_structure_has_native_pointer_sized_layout():
    expected = 40 if ctypes.sizeof(ctypes.c_void_p) == 8 else 28
    assert ctypes.sizeof(INPUT) == expected
