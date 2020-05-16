<?php
namespace Titan\Tests\Feature;

use Illuminate\Foundation\Testing\RefreshDatabase;
use Titan\Tests\TestCase;

class AdminMenuTest extends TestCase {
    use RefreshDatabase;


    public function testPageRedirectsToLoginWhenNotLoggedIn()
    {
        $this->followRedirects = false;

        $this->get(route('admin.menu.index'))
            ->assertRedirect('/login')
            ->assertLocation(route('login'));
    }
}
